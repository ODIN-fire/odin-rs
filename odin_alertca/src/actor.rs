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
use odin_server::spa::{SpaServerMsg,DataAvailable,BroadcastWsMsg};
use crate::{
    get_json_update_msg,
    AlertCaConfig, Camera, AlertCaConnector, CameraStore, errors::{Result,OdinAlertCaError}
};

//--- external messages
#[derive(Debug)] pub struct ExecSnapshotAction( pub DynDataRefAction<CameraStore> );

//--- internal messages (from AlertCaConnector)
#[derive(Debug)] pub struct CameraUpdates(pub(crate) Vec<Camera>);

define_actor_msg_set! { pub AlertCaActorMsg =
    //-- messages we get from other actors
    ExecSnapshotAction |

    //-- messages we get from our connector (note these are not public)
    CameraUpdates
}

pub struct AlertCaActor <C,I,U>
    where C: AlertCaConnector + Send,  I: DataRefAction<CameraStore>,  U: DataRefAction<Vec<Camera>>
{
    config: Arc<AlertCaConfig>,
    connector: C,               // where we get the external data from
    store: Option<CameraStore>, // our internal store (initialized from the first CameraUpdates we get from the connector)

    init_action: I,             // initialized interaction (triggered by self)
    update_action: U,           // update interactions (triggered by self)
}

impl <C,I,U> AlertCaActor <C,I,U>
    where C: AlertCaConnector + Send,  I: DataRefAction<CameraStore>,  U: DataRefAction<Vec<Camera>>
{
    pub fn new (config: AlertCaConfig, connector_ctor: fn(Arc<AlertCaConfig>)->C, init_action: I, update_action: U)->Self {
        let config = Arc::new(config);
        let connector = connector_ctor( config.clone());

        AlertCaActor{config, connector, store: None, init_action, update_action}
    }

    /// the first CameraUpdates we get from the connector (after it was started from _Start_) initialize our store and
    /// trigger the init_action. All subsequent ones are reported through the update_action
    async fn update (&mut self, cameras: Vec<Camera>)->Result<()> {
        if let Some(store) = self.store.as_mut() {
            // since this consumes the updates we have to execute the action first. This is suboptimal
            // but at least would allow to assess changes
            self.update_action.execute( &cameras).await;
            store.update_all( cameras);

        } else {
            let mut store = CameraStore::new( self.config.max_history);
            store.update_all( cameras);
            info!("initialized camera store with {} cameras", store.len());

            self.init_action.execute( &store).await;
            self.store = Some(store);
        }

        Ok(())
    }
}

impl_actor! { match msg for Actor<AlertCaActor<C,I,U>, AlertCaActorMsg>
    where C: AlertCaConnector + Send + Sync,  I: DataRefAction<CameraStore> + Send + Sync,  U: DataRefAction<Vec<Camera>> + Send + Sync
    as

    //--- user messages
    ExecSnapshotAction => cont! {
        if let Some(store) = &self.store { // snapshot requests should only come in after the init_action was executed
            msg.0.execute( store).await;
        } else {
            warn!("ignoring snapshot request for uninitialized camera store");
        }
    }

    CameraUpdates => cont! {
        self.update( msg.0).await;
    }

        //--- system messages
    _Start_ => cont! {
        let hself = self.hself.clone();
        if let Err(e) = self.connector.start( hself).await {  // this should eventually lead to a CameraUpdates that initializes our store
            error!("failed to start connector: {:?}", e)
        }
        // note that we do not clean up our cache dir - this is done by odin_scrub
    }

    _Terminate_ => stop! {
        self.connector.terminate();
    }
}

pub fn server_init_action (hserver: ActorHandle<SpaServerMsg>, actor_id: &str) -> impl DataRefAction<CameraStore> {
    dataref_action!(
        let sender_id: Arc<String> = Arc::new( actor_id.to_string()),
        let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |_store: &CameraStore| {
            Ok( hserver.try_send_msg( DataAvailable::new::<CameraStore>(sender_id) )? )
        }
    )
}

pub fn server_update_action (hserver: ActorHandle<SpaServerMsg>) -> impl DataRefAction<Vec<Camera>> {
    dataref_action!(
        let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => |updates: &Vec<Camera>| {
            let ws_msg = get_json_update_msg( &updates);
            Ok( hserver.try_send_msg( BroadcastWsMsg{ws_msg})? )
        }
    )
}
