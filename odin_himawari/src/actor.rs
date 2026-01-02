/*
 * Copyright © 2025, United States Government, as represented by the Administrator of
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

use std::{sync::Arc};
use odin_actor::prelude::*;
use crate::{
    HimawariConfig, HimawariHotspot, HimawariHotspotSet, HimawariHotspotStore, PKG_CACHE_DIR, errors::{OdinHimawariError,Result}, live_importer::LiveHimawariHotspotImporter
};

pub trait HimawariHotspotImporter {
    fn start (&mut self, hself: ActorHandle<HimawariHotspotActorMsg>) -> impl Future<Output=Result<()>> + Send;
    fn terminate (&mut self);
}

/// external message to request action execution with the current HotspotStore
#[derive(Debug)] pub struct ExecSnapshotAction(pub DynDataRefAction<HimawariHotspotStore>);

// internal messages sent by the HimawariDataImporter
#[derive(Debug)] pub struct Initialize(pub(crate) Vec<HimawariHotspotSet>);
#[derive(Debug)] pub struct Update(pub(crate) HimawariHotspotSet);
#[derive(Debug)] pub struct ImportError(pub(crate) OdinHimawariError);

define_actor_msg_set! { pub HimawariHotspotActorMsg = ExecSnapshotAction | Initialize | Update | ImportError }

/// user part of the Himawari import actor
/// this basically provides a message interface around an encapsulated, async updated HotspotStore
#[derive(Debug)]
pub struct HimawariHotspotActor<T,I,U>
    where T: HimawariHotspotImporter + Send, I: DataRefAction<HimawariHotspotStore>, U: DataAction<HimawariHotspotSet>
{
    hotspot_store: HimawariHotspotStore,
    importer: T,
    init_action: I,
    update_action: U
}

impl <T,I,U> HimawariHotspotActor<T,I,U>
    where T: HimawariHotspotImporter + Send, I: DataRefAction<HimawariHotspotStore>, U: DataAction<HimawariHotspotSet>
{
    pub fn new (config: Arc<HimawariConfig>, importer: T, init_action: I, update_action: U) -> Self {
        let hotspot_store = HimawariHotspotStore::new( config.init_hours * 6); // we assume updates every 10 min

        HimawariHotspotActor{hotspot_store, importer, init_action, update_action}
    }

    pub async fn init (&mut self, mut init_hotspots: Vec<HimawariHotspotSet>) -> Result<()> {
        self.hotspot_store.initialize_hotspots(init_hotspots);
        self.init_action.execute(&self.hotspot_store).await;
        Ok(())
    }

    pub async fn update (&mut self, mut new_hotspots: HimawariHotspotSet) -> Result<()> {
        self.hotspot_store.update_hotspots(new_hotspots.clone());
        self.update_action.execute(new_hotspots).await;
        Ok(())
    }
}

impl_actor! { match msg for Actor< HimawariHotspotActor<T,I,U>, HimawariHotspotActorMsg>
    where T:HimawariHotspotImporter + Send + Sync, I: DataRefAction<HimawariHotspotStore> + Sync, U: DataAction<HimawariHotspotSet> + Sync
    as
    _Start_ => cont! {
        let hself = self.hself.clone();
        self.importer.start( hself).await;
    }

    ExecSnapshotAction => cont! { msg.0.execute( &self.hotspot_store).await; }

    Initialize => cont! { self.init(msg.0).await; }

    Update => cont! { self.update(msg.0).await; }

    ImportError => cont! { error!("{:?}", msg.0); }

    _Terminate_ => stop! { self.importer.terminate(); }
}
