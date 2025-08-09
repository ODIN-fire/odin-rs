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
#![allow(unused,private_interfaces,private_bounds)]

use std::sync::{Arc,atomic::Ordering};
use chrono::{DateTime,Utc};
use odin_actor::prelude::*;
use odin_actor::{error,debug,warn,info};
use odin_common::datetime::EpochMillis;
use dashmap::DashMap;
use crate::{Aircraft, AircraftStore, adsb::{AdsbConfig,AdsbConnector}, errors::{Result,OdinAdsbError}};

//--- external messages
#[derive(Debug)] pub struct ExecSnapshotAction( pub DynDataRefAction<AircraftStore> );

//--- internal messages (from N5Connector)
#[derive(Debug)] pub(crate) struct ConnectorError (pub(crate) OdinAdsbError);

define_actor_msg_set! { pub AdsbActorMsg = 
    //-- messages we get from other actors
    ExecSnapshotAction |

    //-- messages we get from our connector (note these are not public)
    ConnectorError
}

/// actor that imports ADS-B data from an AdsbConnector and published respective Aircraft updates and snapshots
pub struct AdsbActor <C,U> 
    where C: AdsbConnector + Send,  U: DataRefMutAction<AircraftStore>
{
    config: Arc<AdsbConfig>,
    connector: C,                 // where we get the external data from
    timer: Option<AbortHandle>,   // for triggering the update_action (for changed Aircraft)

    store: AircraftStore,         // our internal store

    update_action: U,             // update interactions (triggered by self)
}

impl<C,U> AdsbActor <C,U>
    where C: AdsbConnector + Send,  U: DataRefMutAction<AircraftStore>
{
    pub fn new (config: AdsbConfig, update_action: U)->Self {
        let config = Arc::new(config);
        let store = AircraftStore::new( config.source.clone());
        let connector = C::new( config.clone(), store.timestamp.clone(), store.aircraft.clone());
        AdsbActor { config, connector, timer: None, store, update_action }
    }

    async fn update(&mut self)->Result<()> {
        let ts = EpochMillis::new( self.store.timestamp.load(Ordering::Relaxed)); // updated by connector (make sure we save before awaiting)

        self.store.remove_stale( self.config.drop_after); // remove stale aircraft /before/ executing the update action
        self.update_action.execute( &self.store).await?;

        self.store.set_last_update( ts);
        Ok(())
    }
}

impl_actor! { match msg for Actor<AdsbActor<C,U>, AdsbActorMsg> 
    where C: AdsbConnector + Send + Sync,  U: DataRefMutAction<AircraftStore> + Sync
    as 

    //--- user messages
    ExecSnapshotAction => cont! {
        msg.0.execute( &self.store).await;
    }

    //--- (private) connector messages
    ConnectorError => cont! { 
        error!("connector error: {:?}", msg) // TODO - this needs to be handled
    }

    //--- system messages
    _Start_ => cont! {
        let hself = self.hself.clone();
        if let Err(e) = self.connector.start( hself).await {  // this should eventually lead to an InitializeStore
            error!("failed to start connector: {:?}", e)
        }

        if let Ok(timer) = self.start_repeat_timer( 1, self.config.update_interval, false) {
            self.timer = Some(timer);
            println!("started update timer in '{}'", self.hself.id());
        }
    }

    _Timer_ => cont! { 
        if let Err(e) = self.update().await { 
            error!("update failed: {:?}", e)
        }
    }

    _Terminate_ => stop! { 
        self.connector.terminate(); 
    }
}
