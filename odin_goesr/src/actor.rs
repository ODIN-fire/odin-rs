/*
 * Copyright © 2024, United States Government, as represented by the Administrator of 
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

//! actors for odin_goesr data

use odin_actor::prelude::*;
use crate::*; 

#[derive(Serialize,Deserialize,Debug)]
pub struct GoesrImportActorConfig {
    pub max_records: usize,
}

/// external message to request action execution with the current HotspotStore
#[derive(Debug)] pub struct ExecSnapshotAction(pub DynDataRefAction<GoesrHotspotStore>);

// internal messages sent by the GoesRDataImporter
#[derive(Debug)] pub struct Update(pub(crate) GoesrHotspotSet);
#[derive(Debug)] pub struct Initialize(pub(crate) Vec<GoesrHotspotSet>);
#[derive(Debug)] pub struct ImportError(pub(crate) OdinGoesrError);

define_actor_msg_set! { pub GoesrHotspotImportActorMsg = ExecSnapshotAction | Initialize | Update | ImportError }

/// user part of the GoesR import actor
/// this basically provides a message interface around an encapsulated, async updated HotspotStore
#[derive(Debug)]
pub struct GoesrHotspotActor<T,I,U> 
    where T: GoesrHotspotImporter + Send, I: DataRefAction<GoesrHotspotStore>, U: DataAction<GoesrHotspotSet>
{
    hotspot_store: GoesrHotspotStore,
    goesr_importer: T,
    init_action: I,
    update_action: U
}
 
impl <T,I,U> GoesrHotspotActor<T,I,U> 
    where T: GoesrHotspotImporter + Send, I: DataRefAction<GoesrHotspotStore>, U: DataAction<GoesrHotspotSet>
{
    pub fn new (config: GoesrImportActorConfig, goesr_importer: T, init_action: I, update_action: U) -> Self {
        let hotspot_store = GoesrHotspotStore::new(config.max_records);

        GoesrHotspotActor{hotspot_store, goesr_importer, init_action, update_action}
    }

    pub async fn init (&mut self, init_hotspots: Vec<GoesrHotspotSet>) -> Result<()> {
        self.hotspot_store.initialize_hotspots(init_hotspots.clone());
        self.init_action.execute(&self.hotspot_store).await;
        Ok(())
    }

    pub async fn update (&mut self, new_hotspots: GoesrHotspotSet) -> Result<()> {
        self.hotspot_store.update_hotspots(new_hotspots.clone());
        self.update_action.execute(new_hotspots).await;
        Ok(())
    }
}
 
impl_actor! { match msg for Actor< GoesrHotspotActor<T,I,U>, GoesrHotspotImportActorMsg> 
    where T:GoesrHotspotImporter + Send + Sync, I: DataRefAction<GoesrHotspotStore> + Sync, U: DataAction<GoesrHotspotSet> + Sync
    as
    _Start_ => cont! { 
        let hself = self.hself.clone();
        self.goesr_importer.start( hself).await; 
    }

    ExecSnapshotAction => cont! { msg.0.execute( &self.hotspot_store).await; }

    Initialize => cont! { self.init(msg.0).await; }

    Update => cont! { self.update(msg.0).await; }

    ImportError => cont! { error!("{:?}", msg.0); }
    
    _Terminate_ => stop! { self.goesr_importer.terminate(); }
}

/// abstraction for the data acquisition mechanism used by the GoesRImportActor
/// impl objects are used as GoesRImportActor constructor arguments. It is Ok to panic in the instantiation
pub trait GoesrHotspotImporter {
    fn start (&mut self, hself: ActorHandle<GoesrHotspotImportActorMsg>) -> impl Future<Output=Result<()>> + Send;
    fn terminate (&mut self);
}