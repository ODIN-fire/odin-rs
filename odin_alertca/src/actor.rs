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
#![allow(unused)]

use std::{path::PathBuf, sync::Arc, collections::HashMap};
use odin_actor::{load_config, prelude::*};
use odin_actor::{error,debug,warn,info};
use odin_common::{fs::remove_old_files, datetime::minutes};
use crate::{create_cameras, get_default_cal_oes_cameras, pkg_cache_dir, AlertCaConfig, CalOesCamera, CameraUpdate};
use crate::{AlertCaConnector,CameraStore,errors::{Result,OdinAlertCaError}};

//--- external messages
#[derive(Debug)] pub struct ExecSnapshotAction( pub DynDataRefAction<CameraStore> );

//--- internal messages (from N5Connector)
#[derive(Debug)] pub struct CameraUpdates(pub(crate) Vec<CameraUpdate>);

define_actor_msg_set! { pub AlertCaActorMsg = 
    //-- messages we get from other actors
    ExecSnapshotAction |

    //-- messages we get from our connector (note these are not public)
    CameraUpdates
}

pub struct AlertCaActor <C,I,U> 
    where C: AlertCaConnector + Send,  I: DataRefAction<CameraStore>,  U: DataRefAction<Vec<CameraUpdate>>
{
    config: Arc<AlertCaConfig>,
    connector: C,               // where we get the external data from
    store: CameraStore,         // our internal store
    timer: Option<AbortHandle>,

    init_action: I,             // initialized interaction (triggered by self)
    update_action: U,           // update interactions (triggered by self)
}

impl <C,I,U> AlertCaActor <C,I,U> 
    where C: AlertCaConnector + Send,  I: DataRefAction<CameraStore>,  U: DataRefAction<Vec<CameraUpdate>>
{
    pub fn new (config: AlertCaConfig, connector_ctor: fn(Arc<AlertCaConfig>,Arc<HashMap<String,CalOesCamera>>)->C, init_action: I, update_action: U)->Self {
        let config = Arc::new(config);
        let cal_oes_cameras = Arc::new(get_default_cal_oes_cameras().unwrap()); // Ok to panic here since this is a toplevel object

        let connector = connector_ctor( config.clone(), cal_oes_cameras.clone());
        let store = create_cameras( config.as_ref(), &cal_oes_cameras).unwrap(); // ditto

        AlertCaActor{config, connector, store, timer: None, init_action, update_action}
    }

    async fn update (&mut self, updates: Vec<CameraUpdate>)->Result<()> {
        let is_first_update = self.store.last_update.millis() == 0;
        
        if is_first_update {
            self.store.update_all( updates);
            self.init_action.execute( &self.store).await;
        } else {
            // since this consumes the updates we have to execute the action first. This is suboptimal
            // but at least would allow to assess changes
            self.update_action.execute( &updates).await;
            self.store.update_all( updates); 
        }

        Ok(())
    }

    fn cleanup (&mut self) {
        if remove_old_files( &pkg_cache_dir!(), self.config.max_age).is_err() {
            warn!("failed to cleanup cache");
        }
    }
}

impl_actor! { match msg for Actor<AlertCaActor<C,I,U>, AlertCaActorMsg> 
    where C: AlertCaConnector + Send + Sync,  I: DataRefAction<CameraStore> + Send + Sync,  U: DataRefAction<Vec<CameraUpdate>> + Send + Sync
    as 

    //--- user messages
    ExecSnapshotAction => cont! {
        msg.0.execute( &self.store).await;
    }

    CameraUpdates => cont! {
        self.update( msg.0).await;
    }

        //--- system messages
    _Start_ => cont! {
        let hself = self.hself.clone();
        if let Err(e) = self.connector.start( hself).await {  // this should eventually lead to an InitializeStore
            error!("failed to start connector: {:?}", e)
        }

        if let Ok(timer) = self.start_repeat_timer( 1, minutes(30), true) {
            self.timer = Some(timer);
        } else { error!("failed to start cleanup timer") }
    }

    _Timer_ => cont! {
        self.cleanup();
    }

    _Terminate_ => stop! { 
        self.connector.terminate(); 
    }
}