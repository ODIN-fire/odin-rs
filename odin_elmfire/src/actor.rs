/*
 * Copyright 2026 Peter Mehlitz
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://apache.org
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
#![allow(unused)]

use std::{path::{Path,PathBuf}, str::FromStr, time::Duration, collections::HashMap,
    fs::{File,read_to_string, remove_dir_all}, ops::{Deref,DerefMut}, io::{BufReader,BufWriter,Read,Write},
    sync::Arc, result::{Result as StdResult}
};
use chrono::{DateTime,Utc,Datelike,Timelike};
use derive_more::{Deref,DerefMut};

use odin_actor::prelude::*;
use odin_actor::{error,debug,warn,info};
use odin_common::{fs, json_writer::{JsonWritable, JsonWriter}};
use odin_server::{spa::{SpaServerMsg, SpaService, BroadcastWsMsg}, ws_service::ws_msg_from_json};
use serde::Deserialize;

use crate::{
    CACHE_DIR, EF_RUN_RE, ElmfireConfig, ElmfireOutput, ElmFire, ElmfireRun,
    service::{ElmfireService},
    errors::{OdinElmfireError,Result, op_failed}
};

#[derive(Deref,DerefMut)]
pub struct ElmfireStore(HashMap<String,ElmfireRun>);  // id->ElmfireJob

impl ElmfireStore {
    pub fn get_json_snapshot_msg (&self)->String {
        let mut w = JsonWriter::with_capacity(8192);
        w.write_object( |w| {
            w.write_array_field("elmfires", |w| {
                for er in self.0.values() {
                    er.write_json_to( w);
                }
            })
        });
        ws_msg_from_json( ElmfireService::mod_path(), "snapshot", w.as_str())
    }
}

/// external message to process all currently stored runs
#[derive(Debug)] pub struct ExecSnapshotAction( pub DynDataRefAction<ElmfireStore> );

/// internal notifications about a completed run
#[derive(Debug)] pub struct RequestFinished { id: String, result: StdResult<ElmfireOutput,String> }

#[derive(Debug,Deserialize)]
pub struct RemoveElmFire {
    pub id: String
}

define_actor_msg_set! { pub ElmfireActorMsg = ExecSnapshotAction | ElmFire | RemoveElmFire | RequestFinished }

pub struct ElmfireActor <U,R>
    where U: DataRefAction<ElmfireRun> + Send + Sync, R: DataRefAction<ElmfireRun> + Send + Sync
{
    config: Arc<ElmfireConfig>,
    store: ElmfireStore,       // our internal store

    update_action: U,           // store update interactions (triggered by self)
    remove_action: R            // remove elmfire interactions
}

impl <U,R> ElmfireActor <U,R>
    where U: DataRefAction<ElmfireRun> + Send + Sync, R: DataRefAction<ElmfireRun> + Send + Sync
{
    pub fn new (config: ElmfireConfig, update_action: U, remove_action: R)->Self {
        let mut store: ElmfireStore = ElmfireStore(HashMap::new());
        let config = Arc::new(config);

        reset_store( &mut store);
        ElmfireActor{ config, store, update_action, remove_action }
    }

    async fn process_request (&mut self, hself: ActorHandle<ElmfireActorMsg>, fire: ElmFire)->Result<()> {
        info!("elmfire actor got request: {}", fire.id);

        let run = ElmfireRun {
            fire: Arc::new( fire ),
            result: None
        };
        let id = run.fire.id.clone();

        if self.store.contains_key( &id) {
            return Err( op_failed!("already stored: {}", &id));
        } else {
            self.store.insert( id.clone(), run.clone()); // cloning the job is inexpensive
            self.update_action.execute( &run).await.map_err(|e| OdinElmfireError::ActionFailure(e.to_string()))?;

            // we don't await completion of the (possibly lengthy) run here so that we stay responsive
            let config = self.config.clone();

            spawn( "elmfire executor", async move {
                info!("executing {}", id);
                let result = run.fire.execute( config.as_ref()).await.map_err( |e| e.to_string());
                hself.send_msg( RequestFinished{ id, result }).await;
            });

            Ok(())
        }
    }

    async fn process_result (&mut self, id: String, result: StdResult<ElmfireOutput,String>)->Result<()> {
        if let Some(run) = self.store.get_mut( &id) {
            let success = result.is_ok();
            info!("processing result for {} with status {}", id, success);
            run.result = Some(result);

            if success { // we only store successful runs
                let path = CACHE_DIR.join(&id).join("elmfire.ron");
                match run.save_to(&path) {
                    Ok(()) => info!("elmfire run saved to {:?}", path),
                    Err(e) => warn!("failed to save elmfire run: {}", e)
                }
            }

            self.update_action.execute( &run).await.map_err(|e| OdinElmfireError::ActionFailure(e.to_string()))?;
            Ok(())
        } else {
            Err( op_failed!("ignoring unknown run: {}", id) )
        }
    }

    async fn remove_elmfire (&mut self, id: &str)->Result<()> {
        if let Some(run) = self.store.remove(id) {
            info!("removing run {}", id);
            // note we leave it up to the action what to do with the associated cache dir
            self.remove_action.execute( &run).await.map_err(|e| OdinElmfireError::ActionFailure(e.to_string()))?;
            Ok(())

        } else {
            Err( op_failed!("ignoring remove request for unknown run: {}", id) )
        }
    }
}

fn reset_store (store: &mut ElmfireStore) {
    store.clear();

    if let Ok(ef_paths) = fs::matching_files_in_tree( CACHE_DIR.as_path(), &EF_RUN_RE) {
        for path in ef_paths {
            if let Ok(ef) = ElmfireRun::read_from(&path) {
                let id = ef.fire.id.clone();
                info!("loaded Elmfire run from cache: {}", id);
                store.insert( id, ef);
            }
        }
    }
}

impl_actor! { match msg for Actor<ElmfireActor<U,R>, ElmfireActorMsg>
    where U: DataRefAction<ElmfireRun> + Send + Sync,  R: DataRefAction<ElmfireRun> + Send + Sync
    as

    ExecSnapshotAction => cont! {
        reset_store( &mut self.store);   // FIXME - this is a hot fix, we need a proper reset mechanism

        msg.0.execute( &self.store).await;
    }

    // received from external (clients)
    ElmFire => cont! {
        let hself = self.hself.clone();
        if let Err(e) = self.process_request( hself, msg).await {
            error!("process request failed: {}", e)
        }
    }

    RequestFinished => cont! {
        if let Err(e) = self.process_result( msg.id, msg.result).await {
            error!("failed to process result: {}", e)
        }
    }

    RemoveElmFire => cont! {
        if let Err(e) = self.remove_elmfire( &msg.id).await {
            error!("remove elmfire {} failed: {}", msg.id, e)
        }
    }
}

// standard actions in a SpaServer context

pub fn server_update_action (hserver: ActorHandle<SpaServerMsg>) -> impl DataRefAction<ElmfireRun> {
    dataref_action!(
        let hserver: ActorHandle<SpaServerMsg> = hserver => |run: &ElmfireRun| {
            let ws_msg = run.get_json_update_msg();
            Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
        }
    )
}

pub fn server_remove_action (hserver: ActorHandle<SpaServerMsg>) -> impl DataRefAction<ElmfireRun> {
    dataref_action!(
        let hserver: ActorHandle<SpaServerMsg> = hserver => |run: &ElmfireRun| {
            run.remove_dir()?;
            let ws_msg = run.get_json_remove_msg();
            Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
        }
    )
}
