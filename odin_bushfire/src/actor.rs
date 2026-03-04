/*
 * Copyright © 2026, United States Government, as represented by the Administrator of
 * the National Aeronautics and Space Administration. All rights reserved.
 *
 * The “ODIN” software is licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License. You may obtain a copy
 * of the License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software distributed under
 * the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
 * either express or implied. See the License for the specific language governing permissions
 * and limitations under the License.
 */
#![allow(unused)]

use std::collections::{HashMap, VecDeque};
use reqwest::Client;
use chrono::{DateTime,Utc,Datelike,Timelike};

use odin_actor::prelude::*;
use odin_actor::{error,debug,warn,info};
use odin_common::{collections::RingDeque,fs::remove_old_files};

use crate::fill_in_position_heights;
use crate::{
    BushFireConfig, Bushfire, BushfireStore, CACHE_DIR,
    download_file, get_features, cleanup_feature_properties,
    get_bushfires, errors::{Result,OdinBushfireError,op_failed}
};

#[derive(Debug)] pub struct ExecSnapshotAction( pub DynDataRefAction<BushfireStore> );

define_actor_msg_set! { pub BushfireActorMsg = ExecSnapshotAction }

pub struct BushfireActor <I,U>
    where I: DataRefAction<BushfireStore>,  U: DataAction<Vec<Bushfire>>
{
    config: BushFireConfig,
    store: BushfireStore,       // our internal store

    init_action: I,             // initialized interaction (triggered by self)
    update_action: U,           // update interactions (triggered by self)

    client: Client,
    n_updates: usize,
    timer: Option<AbortHandle>,
}

impl <I,U> BushfireActor <I,U>
    where I: DataRefAction<BushfireStore>,  U: DataAction<Vec<Bushfire>>
{
    pub fn new (config: BushFireConfig, init_action: I, update_action: U)->Self {
        let store: BushfireStore = BushfireStore(HashMap::new());
        let client = Client::new();
        BushfireActor{ config, store, init_action, update_action, client, n_updates: 0, timer: None }
    }

    async fn initialize (&mut self, hself: ActorHandle<BushfireActorMsg>) {
        // do initial update and slot exec here

        if let Ok(timer) = hself.start_repeat_timer( 1, self.config.check_interval, false) {
            self.timer = Some(timer);
        } else { error!("failed to bushfire timer") }
    }

    fn add_new_bushfire (&mut self, f: Bushfire) {
        let mut hist = VecDeque::with_capacity(self.config.max_history);
        let fire_id = f.id.clone();
        hist.push_back(f);
        self.store.insert( fire_id, hist);
    }

    async fn update (&mut self)->Result<()> {
        remove_old_files( &*CACHE_DIR, self.config.max_file_age);

        let path = download_file( &self.client, &self.config.url, Utc::now()).await?;
        if path.is_file() {
            let mut features = get_features(&path)?;
            cleanup_feature_properties(&mut features);
            let mut bushfires = get_bushfires( &features, Some(CACHE_DIR.as_path()), Some(self.config.max_age))?;

            if let Some(dem) = &self.config.dem {
                fill_in_position_heights( &mut bushfires, dem).await?;
            }

            if self.n_updates == 0 { // this is our initialization
                for f in bushfires {
                    self.add_new_bushfire(f);
                }
                self.init_action.execute( &self.store).await;

            } else { // subsequent update
                let mut updates: Vec<Bushfire> = Vec::new();
                for f in bushfires {
                    if let Some(hist) = self.store.get_mut( &f.id) {
                        if let Some(last_entry) = hist.back() {
                            if last_entry.date < f.date { // this is an update
                                hist.push_to_ringbuffer(f.clone());
                                updates.push(f);
                            }
                        }
                    } else {
                        self.add_new_bushfire(f.clone());
                        updates.push( f);
                    }
                }
                self.update_action.execute( updates).await;
            }

            self.n_updates += 1;

            Ok(())
        } else {
            Err ( op_failed!("no snapshot file") )
        }
    }
}

impl_actor! { match msg for Actor<BushfireActor<I,U>, BushfireActorMsg>
    where I: DataRefAction<BushfireStore> + Sync,  U: DataAction<Vec<Bushfire>> + Sync
    as

    ExecSnapshotAction => cont! {
        msg.0.execute( &self.store).await;
    }

    _Start_ => cont! {
        let hself = self.hself.clone();
        self.initialize(hself).await;
    }

    _Timer_ => cont! {
        self.update().await;
    }
}
