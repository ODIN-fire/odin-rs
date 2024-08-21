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
#![allow(unused)]

use std::net::SocketAddr;
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade, CloseFrame},
    response::{Response,IntoResponse},
    routing::get,
    Router,
    extract::connect_info::ConnectInfo
};
use futures::{sink::SinkExt, stream::StreamExt};

use crate::{
    asset_uri, load_asset, self_crate, spa::{AddConnection, SpaComponents, SpaServerState, SpaService}, OdinServerResult
};

/// a SpaService that adds a shared websocket for all services that register for it
/// this mostly adds a route for the websocket and adds a respective JS module
pub struct WsService {
    // tbd
}

impl WsService {
    pub fn new()->Self { WsService{} }
}

impl SpaService for WsService {
    fn add_components (&self,spa: &mut SpaComponents) -> OdinServerResult<()> {
        spa.add_assets( self_crate!(), load_asset);
        spa.add_module( asset_uri!("ws.js"));

        spa.add_route( |router: Router, spa_server_state: SpaServerState| {
            router.route( &format!("/{}/ws", spa_server_state.name.as_str()), get( {
                let state = spa_server_state.clone();
                move |ws: WebSocketUpgrade, ci: ConnectInfo<SocketAddr>| { ws_handler(ws, ci, state) }
            }))
        });

        Ok(())
    }
}

async fn ws_handler (ws: WebSocketUpgrade, ConnectInfo(addr): ConnectInfo<SocketAddr>, sss: SpaServerState)->Response {
    ws.on_upgrade( move |socket| handle_socket(socket, addr, sss)).into_response()
}

async fn handle_socket(mut ws: WebSocket, remote_addr: SocketAddr, sss: SpaServerState) {
    sss.hself.send_msg( AddConnection{remote_addr,ws}).await;
}

/* #region WsMsg serialization  *******************************************************************************/

// re-export since it is used in the define_ws_struct implementation
pub extern crate serde;

use serde::{Serialize,ser::{Serializer,SerializeStruct}};
use serde_json;
use std::any::type_name;


pub struct WsMsg<T> where T: Serialize {
    pub js_module: String,
    pub payload: T
}

impl <T>  WsMsg<T> where T: Serialize {
    pub fn new (js_module: impl ToString, payload: T)->Self { 
        WsMsg {
            js_module: js_module.to_string(),
            payload
        }
    }
}

impl <T> Serialize for WsMsg<T> where T: Serialize {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut ft = type_name::<T>();
        if let Some(idx) = ft.rfind(':') {
            ft = &ft[idx+1..];
        }

        let mut state = serializer.serialize_struct("WsMsg", 2)?;
        state.serialize_field("mod", &self.js_module)?;
        state.serialize_field(ft, &self.payload)?;
        state.end()
    }
}

pub fn to_json<T> (js_module: impl ToString, payload: T)->OdinServerResult<String> where T: Serialize {
    let ws_msg = WsMsg::new( js_module, payload);
    Ok(serde_json::to_string(&ws_msg)?)
}

/// syntactic sugar for structs we want to send to web sockets
/// this is not providing additional features like the general define_struct - it just adds the required serde macros
/// and uses the serde re-export from odin_server so that callers don't have to take care of it
#[macro_export]
macro_rules! define_ws_struct {
    ($svis:vis $sname:ident = $( $fvis:vis $fname:ident : $ftype:ty ),*) => {
        #[derive(serde::Serialize)]
        #[serde(rename_all="camelCase")]
        $svis struct $sname {
            $( $fvis $fname: $ftype,)*
        }
    }
}
pub use define_ws_struct;

/* #endregion WsMsg serialization */