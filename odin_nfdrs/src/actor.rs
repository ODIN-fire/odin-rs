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

use std::{collections::{HashMap,VecDeque}, path::PathBuf, sync::Arc, result::Result as StdResult};
use derive_more::{Deref,DerefMut};
use serde::Deserialize;
use uom::si::length::meter;

use odin_actor::prelude::*;
use odin_actor::{error,debug,warn,info};
use odin_gdal::geods::GeoDs;
use odin_openmeteo::{OpenMeteoConfig, ids::ModelId};
use odin_server::{spa::{SpaServerMsg, SpaService, BroadcastWsMsg}, ws_service::ws_msg_from_json};

use crate::{
    GRID_DIR, NfdrsConfig,
    grid::{NfdrsOutput, NfdrsRequest, NfdrsRun, NfdrsRunMsg, compute_nfdrs_output},
    service::NfdrsService,
    errors::{OdinNfdrsError, Result, op_failed}
};

/// the store of all known NfdrsRuns (id -> run)
#[derive(Deref,DerefMut)]
pub struct NfdrsStore(HashMap<String,NfdrsRun>);

impl NfdrsStore {
    pub fn get_json_snapshot_msg (&self)->String {
        let mut runs: Vec<&NfdrsRun> = self.0.values().collect();
        runs.sort_by( |a,b| b.request.date.cmp( &a.request.date)); // most recent first
        let msgs: Vec<NfdrsRunMsg> = runs.iter().map( |r| r.to_msg()).collect();
        let json = serde_json::to_string( &msgs).unwrap_or_else( |_| "[]".to_string());
        ws_msg_from_json( NfdrsService::mod_path(), "snapshot", &json)
    }
}

impl NfdrsRun {
    pub fn get_json_update_msg (&self)->String {
        let json = serde_json::to_string( &self.to_msg()).unwrap_or_else( |_| "{}".to_string());
        ws_msg_from_json( NfdrsService::mod_path(), "update", &json)
    }

    pub fn get_json_remove_msg (&self)->String {
        let json = format!(r#"{{"id":"{}"}}"#, self.id());
        ws_msg_from_json( NfdrsService::mod_path(), "remove", &json)
    }
}

/// external message to process all currently stored runs
#[derive(Debug)] pub struct ExecSnapshotAction( pub DynDataRefAction<NfdrsStore> );

/// external request to remove a (non-running) run
#[derive(Debug,Deserialize)]
pub struct RemoveNfdrsRun {
    pub id: String
}

/// internal notification about a completed run
#[derive(Debug)] pub struct RunFinished { id: String, result: StdResult<NfdrsOutput,String> }

define_actor_msg_set! { pub NfdrsActorMsg = ExecSnapshotAction | NfdrsRequest | RemoveNfdrsRun | RunFinished }

/// the immutable data we need to compute NfdrsRuns (shared with computation tasks)
struct NfdrsContext {
    nfdrs_config: NfdrsConfig,
    wx_config: OpenMeteoConfig,
    wx_model: ModelId,
    lf_src: GeoDs,
    root_dir: PathBuf
}

/// actor that processes NfdrsRequests and keeps the respective NfdrsRuns in a NfdrsStore.
///
/// Since NFDRS executions for overlapping grids can share files (and NFDRS computation for larger grids is
/// expensive) we only compute one run at a time. Additional requests are queued.
///
/// TODO - requests with `update` set should be re-computed each time a new forecast becomes available. So far
/// we only keep the flag
pub struct NfdrsActor <U,R>
    where U: DataRefAction<NfdrsRun> + Send + Sync, R: DataRefAction<NfdrsRun> + Send + Sync
{
    ctx: Arc<NfdrsContext>,
    store: NfdrsStore,
    queue: VecDeque<String>,   // ids of pending runs that are not yet running
    running: Option<String>,   // id of the run that is currently computed

    update_action: U,          // store update interactions (triggered by self)
    remove_action: R           // run remove interactions
}

impl <U,R> NfdrsActor <U,R>
    where U: DataRefAction<NfdrsRun> + Send + Sync, R: DataRefAction<NfdrsRun> + Send + Sync
{
    pub fn new (nfdrs_config: NfdrsConfig, wx_config: OpenMeteoConfig, wx_model: ModelId, lf_src: GeoDs, update_action: U, remove_action: R)->Self {
        let root_dir = GRID_DIR.clone();
        let store = load_store( &root_dir);
        let ctx = Arc::new( NfdrsContext{ nfdrs_config, wx_config, wx_model, lf_src, root_dir });

        NfdrsActor{ ctx, store, queue: VecDeque::new(), running: None, update_action, remove_action }
    }

    /// the grid resolution we use for requests (there is no point in using a higher resolution than the wx_model provides)
    pub fn grid_res_m (&self)->f64 {
        self.ctx.wx_model.grid_resolution().get::<meter>()
    }

    async fn process_request (&mut self, hself: ActorHandle<NfdrsActorMsg>, mut request: NfdrsRequest)->Result<()> {
        request.normalize( self.grid_res_m());
        let id = request.id.clone();
        info!("NFDRS actor got request: {}", id);

        if let Some(run) = self.store.get( &id) {
            if !matches!( run.result, Some(Err(_))) { // already done or pending - just re-announce it
                self.update_action.execute( run).await.map_err(|e| OdinNfdrsError::ActionFailure(e.to_string()))?;
                return Ok(())
            }
        }

        // new (or previously failed) run
        let run = NfdrsRun::new( request);
        if let Err(e) = run.save( &self.ctx.root_dir) { warn!("failed to save NFDRS run {}: {}", id, e) }
        self.update_action.execute( &run).await.map_err(|e| OdinNfdrsError::ActionFailure(e.to_string()))?;

        self.store.insert( id.clone(), run);
        self.queue.push_back( id);
        self.start_next( hself);

        Ok(())
    }

    /// start computation of the next queued run if we are not already computing one
    fn start_next (&mut self, hself: ActorHandle<NfdrsActorMsg>) {
        if self.running.is_none() {
            while let Some(id) = self.queue.pop_front() {
                if let Some(run) = self.store.get( &id) { // might have been removed in the meantime
                    let request = run.request.clone(); // the spawned task needs its own copy
                    let ctx = self.ctx.clone();
                    self.running = Some( id.clone());

                    // we don't await completion of the (possibly lengthy) run here so that we stay responsive
                    let res = spawn( "nfdrs executor", async move {
                        info!("computing NFDRS run {}", id);
                        let result = compute_nfdrs_output( &request, &ctx.root_dir,
                                                           &ctx.nfdrs_config, &ctx.wx_config, ctx.wx_model, &ctx.lf_src).await
                                                         .map_err( |e| e.to_string());
                        hself.send_msg( RunFinished{ id, result }).await;
                    });

                    if let Err(e) = res {
                        error!("failed to spawn NFDRS computation: {}", e);
                        self.running = None;
                    }
                    return
                }
            }
        }
    }

    async fn process_result (&mut self, hself: ActorHandle<NfdrsActorMsg>, id: String, result: StdResult<NfdrsOutput,String>)->Result<()> {
        if self.running.as_ref() == Some(&id) { self.running = None }

        let res = if let Some(run) = self.store.get_mut( &id) {
            match &result {
                Ok(_) => info!("NFDRS run {} done", id),
                Err(e) => warn!("NFDRS run {} failed: {}", id, e)
            }
            run.result = Some(result);
            if let Err(e) = run.save( &self.ctx.root_dir) { warn!("failed to save NFDRS run {}: {}", id, e) }

            self.update_action.execute( run).await.map_err(|e| OdinNfdrsError::ActionFailure(e.to_string()))

        } else {
            Err( op_failed!("ignoring result for unknown NFDRS run: {}", id) )
        };

        self.start_next( hself);
        res
    }

    async fn remove_run (&mut self, id: &str)->Result<()> {
        if self.running.as_deref() == Some(id) {
            return Err( op_failed!("cannot remove running NFDRS run {}", id))
        }

        if let Some(run) = self.store.remove(id) {
            info!("removing NFDRS run {}", id);
            self.queue.retain( |qid| qid != id);
            // note we leave it up to the action what to do with the associated dir
            self.remove_action.execute( &run).await.map_err(|e| OdinNfdrsError::ActionFailure(e.to_string()))?;
            Ok(())

        } else {
            Err( op_failed!("ignoring remove request for unknown NFDRS run: {}", id) )
        }
    }
}

/// load persisted runs. Runs that are still pending were interrupted (e.g. by a server shutdown)
fn load_store (root_dir: &PathBuf)->NfdrsStore {
    let mut store = NfdrsStore( HashMap::new());

    for mut run in NfdrsRun::load_all( root_dir) {
        if run.is_pending() {
            run.result = Some( Err("interrupted".to_string()));
            if let Err(e) = run.save( root_dir) { warn!("failed to update interrupted NFDRS run {}: {}", run.id(), e) }
        }
        info!("loaded NFDRS run from cache: {}", run.id());
        store.insert( run.id().to_string(), run);
    }

    store
}

impl_actor! { match msg for Actor<NfdrsActor<U,R>, NfdrsActorMsg>
    where U: DataRefAction<NfdrsRun> + Send + Sync,  R: DataRefAction<NfdrsRun> + Send + Sync
    as

    ExecSnapshotAction => cont! {
        msg.0.execute( &self.store).await;
    }

    // received from external (clients)
    NfdrsRequest => cont! {
        let hself = self.hself.clone();
        if let Err(e) = self.process_request( hself, msg).await {
            error!("process NFDRS request failed: {}", e)
        }
    }

    RunFinished => cont! {
        let hself = self.hself.clone();
        if let Err(e) = self.process_result( hself, msg.id, msg.result).await {
            error!("failed to process NFDRS result: {}", e)
        }
    }

    RemoveNfdrsRun => cont! {
        if let Err(e) = self.remove_run( &msg.id).await {
            error!("remove NFDRS run {} failed: {}", msg.id, e)
        }
    }
}

// standard actions in a SpaServer context

pub fn server_update_action (hserver: ActorHandle<SpaServerMsg>) -> impl DataRefAction<NfdrsRun> {
    dataref_action!(
        let hserver: ActorHandle<SpaServerMsg> = hserver => |run: &NfdrsRun| {
            let ws_msg = run.get_json_update_msg();
            Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
        }
    )
}

pub fn server_remove_action (hserver: ActorHandle<SpaServerMsg>) -> impl DataRefAction<NfdrsRun> {
    dataref_action!(
        let hserver: ActorHandle<SpaServerMsg> = hserver => |run: &NfdrsRun| {
            run.remove_dir( GRID_DIR.as_path())?;
            let ws_msg = run.get_json_remove_msg();
            Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
        }
    )
}
