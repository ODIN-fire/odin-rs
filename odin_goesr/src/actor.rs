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
 
 #[derive(Debug)] pub struct Update(pub(crate) GoesRHotSpots);
 #[derive(Debug)] pub struct Initialize(pub(crate) Vec<GoesRHotSpots>);
 define_actor_msg_set! { pub GoesRActorMsg = Initialize | Update }

 
  pub trait InitAction = DataAction<Vec<GoesRHotSpots>>;
  pub trait UpdateAction = DataAction<GoesRHotSpots>;

  
 #[derive(Debug)]
 pub struct GoesRImportActor<T, A1, A2> where T: GoesRDataImporter + Send, A1:InitAction, A2: UpdateAction {
    hotspot_store: HotspotStore,
    goesr_importer: T,
    task: Option<JoinHandle<()>>,
    init_action: A1,
    update_action: A2
 }
 
 impl <T, A1, A2>GoesRImportActor<T, A1, A2> where T: GoesRDataImporter + Send, A1:InitAction, A2: UpdateAction {
    pub async fn new(config: GoesRImportActorConfig, importer:T, init_action:A1, update_action: A2) -> Self {
        // Set up hotspot store
        let capacity = config.max_records.clone();
        let hotspot_store = HotspotStore::new(capacity);
        GoesRImportActor {
            hotspot_store: hotspot_store,
            goesr_importer: importer,
            task: None,
            init_action: init_action,
            update_action: update_action
        }
    }

    pub async fn init(&mut self, init_hotspots: Vec<GoesRHotSpots>) -> Result<()> {
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
 

 impl_actor! { match msg for Actor<GoesRImportActor<T, A1, A2>,GoesRActorMsg> 
    where T:GoesRDataImporter + Send + Sync, A1: InitAction + Sync, A2: UpdateAction + Sync
    as
    _Start_ => cont! { 
        let hself = self.hself.clone(); 
        self.goesr_importer.start(hself).await;
    }

    Initialize => cont! {
        println!("got initial hotspots");
        self.init(msg.0).await;
        //self.initialize_hotspots(msg.0);
    }

    Update => {
        println!("got updated hotspots");
        self.update(msg.0).await;
        //self.update_hotspots(msg.0);
        ReceiveAction::RequestTermination
    }

    _Terminate_ => stop! {
        self.goesr_importer.terminate();
    }
 }
 
 pub trait GoesRDataImporter {
    fn start (&mut self, hself: ActorHandle<GoesRActorMsg>) -> impl Future<Output=Result<()>> + Send;
    fn terminate (&mut self);
}