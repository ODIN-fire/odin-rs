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

use odin_actor::prelude::*;
use odin_actor::{error,debug,warn,info};
use crate::{N5Connector,DeviceStore,DeviceUpdate,Data,errors::OdinN5Error};

//--- external messages
#[derive(Debug)] pub struct ExecSnapshotAction( pub DynDataRefAction<DeviceStore> );

//--- internal messages (from N5Connector)
#[derive(Debug)] pub(crate) struct InitializeStore (pub(crate) DeviceStore);  // set initial store contents
#[derive(Debug)] pub(crate) struct UpdateStore { // single record update (triggered by websocket notification)
    pub(crate) device_id:u32, 
    pub(crate) update:DeviceUpdate
} 
#[derive(Debug)] pub(crate) struct ConnectorError (pub(crate) OdinN5Error);

define_actor_msg_set! { pub N5ActorMsg = 
    //-- messages we get from other actors
    ExecSnapshotAction |

    //-- messages we get from our connector (note these are not public)
    InitializeStore |
    UpdateStore |
    ConnectorError
}

pub struct N5Actor <C,I,U> 
    where C: N5Connector + Send,  I: DataRefAction<DeviceStore>,  U: DataAction<UpdateStore>
{
    connector: C,               // where we get the external data from
    devices: DeviceStore,       // our internal store

    init_action: I,             // initialized interaction (triggered by self)
    update_action: U,           // update interactions (triggered by self)
}

impl<C,I,U> N5Actor <C,I,U>
    where C: N5Connector + Send, I: DataRefAction<DeviceStore>, U: DataAction<UpdateStore>
{
    pub fn new (connector: C, init_action: I, update_action: U)->Self {
        N5Actor { connector, devices: DeviceStore::new(), init_action, update_action }
    }
}

impl_actor! { match msg for Actor<N5Actor<C,I,U>, N5ActorMsg> 
    where C: N5Connector + Send + Sync,  I: DataRefAction<DeviceStore> + Sync,  U: DataAction<UpdateStore> + Sync
    as 

    //--- user messages
    ExecSnapshotAction => cont! {
        msg.0.execute( &self.devices).await;
    }

    //--- (private) connector messages
    InitializeStore => cont! { 
        //self.init_store( msg.0).await;
    }
    UpdateStore => cont! { 
        //self.update( msg.0).await; 
    }
    ConnectorError => cont! { 
        error!("connector error: {:?}", msg) // TODO - this needs to be handled
    }

    //--- system messages
    _Start_ => cont! {
        let hself = self.hself.clone();
        if let Err(e) = self.connector.start( hself).await {  // this should eventually lead to an InitializeStore
            error!("failed to start connector: {:?}", e)
        }
    }
    _Terminate_ => stop! { 
        self.connector.terminate(); 
    }
}