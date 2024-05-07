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

 use std::collections::VecDeque;

 use odin_actor::prelude::*;
 use crate::*;
 
 #[derive(Debug)] pub struct Update(pub(crate) GoesRHotSpots);
 #[derive(Debug)] pub struct Initialize(pub(crate) Vec<GoesRHotSpots>);
 define_actor_msg_set! { pub GoesRActorMsg = Initialize | Update }
 /*
 Refactor:
 - goesr import actor
 - goesr data acquisition task - trait, start, send cmd, terminate, max_history, data dir
 - live goesr data aquisition task impls above trait, has config, connection, data_dir
 - live task - actual task
  */
 
 #[derive(Clone, Debug)]
 //to do: Add generic types, T goesrdata acquisition task, init/update actions?
 //add: update -> msg reciver send json of updated hotspots
//      initialize -> msg reciever send json of init hotspots
//      snapshot -> msg resever send snapshot
 pub struct GoesRImportActor<T> where T: GoesRDataImporter + Send {
    pub config:GoesRImportActorConfig,
    pub hotspot_store: HashMap<String, VecDeque<GoesRHotSpots>>,
    pub task: T
 }
 
 impl <T>GoesRImportActor<T> where T: GoesRDataImporter + Send {
    pub async fn new(config: GoesRImportActorConfig, task:T) -> Self {
        // Set up hotspot store
        let capacity = config.max_records.clone();
        let hotspot_store: HashMap<String, VecDeque<GoesRHotSpots>> = config.products.iter().map(|x| (x.name.clone(), VecDeque::with_capacity(capacity))).collect();
        GoesRImportActor {
            config: config,
            hotspot_store: hotspot_store,
            task: task
        }
    }
    pub fn update_hotspots(&mut self, new_hotspots: GoesRHotSpots) -> () {
        // if vec is not max add in - assume update is from newer date
        println!("in update");
    let hs_source = new_hotspots.source.clone();
    match self.hotspot_store.get_mut(&hs_source) {
        Some(hotspots) => {
            if hotspots.len() < self.config.max_records {
                hotspots.push_front(new_hotspots);
            } else {
                // remove last, add newest
                hotspots.pop_back();
                hotspots.push_front(new_hotspots);
            }
            
        },
        None => {
            let mut new_hs_vec = VecDeque::with_capacity(self.config.max_records.clone());
            new_hs_vec.push_front(new_hotspots);
            self.hotspot_store.insert(hs_source, new_hs_vec);
        }
    }
    }

    pub fn initialize_hotspots(&mut self, init_hotspots: Vec<GoesRHotSpots>) -> () {
    for hs in init_hotspots {
        match self.hotspot_store.get_mut(&hs.source.clone()) {
            Some(hotspots) => {
                hotspots.push_front(hs);
            },
            None => {
                let source = hs.source.clone();
                let mut new_hs_vec = VecDeque::with_capacity(self.config.max_records.clone());
                new_hs_vec.push_front(hs);
                self.hotspot_store.insert(source, new_hs_vec);
            }
        }
    }
    }
    
 }
 

 impl_actor! { match msg for Actor<GoesRImportActor<T>,GoesRActorMsg> 
    where T:GoesRDataImporter + Send + Sync + Clone
    as
    _Start_ => cont! { // TODO: add non-critical error handling -> error!()/ warning!() 
        let hself = self.hself.clone(); 
        let mut task = self.task.clone();
        let data_task = spawn( "goesr-data-acquisition", async move {
            let _ = task.start(hself).await;
        }
        );
    }

    Initialize => cont! {
        println!("got initial hotspots");
        self.initialize_hotspots(msg.0);
    }

    Update => {
        println!("got updated hotspots");
        self.update_hotspots(msg.0);
        ReceiveAction::RequestTermination
    }
 }
 