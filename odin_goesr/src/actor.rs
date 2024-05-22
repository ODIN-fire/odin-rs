/*
 * Copyright (c) 2024, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The ODIN - Open Data Integration Framework is licensed under the
 * Apache License, Version 2.0 (the "License"); you may not use this file
 * except in compliance with the License. You may obtain a copy of the
 * License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use odin_actor::prelude::*;
use crate::*; 

#[derive(Serialize,Deserialize,Debug)]
pub struct GoesRImportActorConfig {
    pub max_records: usize,
}

/// external message to request action execution with the current HotspotStore
#[derive(Debug)] pub struct ExecSnapshotAction(DynDataRefAction<HotspotStore>);

// internal messages sent by the GoesRDataImporter
#[derive(Debug)] pub struct Update(pub(crate) GoesRHotSpots);
#[derive(Debug)] pub struct Initialize(pub(crate) Vec<GoesRHotSpots>);
#[derive(Debug)] pub struct ImportError(pub(crate) OdinGoesRError);

define_actor_msg_set! { pub GoesRActorMsg = ExecSnapshotAction | Initialize | Update | ImportError }

/// user part of the GoesR import actor
/// this basically provides a message interface around an encapsulated, async updated HotspotStore
#[derive(Debug)]
pub struct GoesRImportActor<T, InitAction, UpdateAction> 
    where T: GoesRDataImporter + Send, 
          InitAction: DataAction<Vec<GoesRHotSpots>>, 
          UpdateAction: DataAction<GoesRHotSpots>
{
    hotspot_store: HotspotStore,
    goesr_importer: T,
    init_action: InitAction,
    update_action: UpdateAction
}
 
impl <T,InitAction,UpdateAction> GoesRImportActor<T, InitAction, UpdateAction> 
    where T: GoesRDataImporter + Send, 
          InitAction: DataAction<Vec<GoesRHotSpots>>, 
          UpdateAction: DataAction<GoesRHotSpots>
{
    pub fn new (config: GoesRImportActorConfig, goesr_importer:T, init_action:InitAction, update_action: UpdateAction) -> Self {
        let hotspot_store = HotspotStore::new(config.max_records);

        GoesRImportActor{hotspot_store, goesr_importer, init_action, update_action}
    }

    pub async fn init (&mut self, init_hotspots: Vec<GoesRHotSpots>) -> Result<()> {
        self.hotspot_store.initialize_hotspots(init_hotspots.clone());
        self.init_action.execute(init_hotspots).await;
        Ok(())
    }

    pub async fn update (&mut self, new_hotspots: GoesRHotSpots) -> Result<()> {
        self.hotspot_store.update_hotspots(new_hotspots.clone());
        self.update_action.execute(new_hotspots).await;
        Ok(())
    }
}
 
impl_actor! { match msg for Actor< GoesRImportActor<T,InitAction,UpdateAction>, GoesRActorMsg> 
    where T:GoesRDataImporter + Send + Sync, 
          InitAction: DataAction<Vec<GoesRHotSpots>> + Sync,
          UpdateAction: DataAction<GoesRHotSpots> + Sync
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
pub trait GoesRDataImporter {
    fn start (&mut self, hself: ActorHandle<GoesRActorMsg>) -> impl Future<Output=Result<()>> + Send;
    fn terminate (&mut self);
}