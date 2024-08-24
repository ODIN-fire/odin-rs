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
    routing::{Router,get},
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

        spa.add_route( |router, spa_server_state| {
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
    pub crate_name: &'static str,
    pub js_module: &'static str,
    pub payload_name: &'static str,
    pub payload: T
}

impl <T>  WsMsg<T> where T: Serialize {
    pub fn new (crate_name: &'static str, js_module: &'static str, payload_name: &'static str, payload: T)->Self { 
        WsMsg {crate_name, js_module, payload_name, payload}
    }

    pub fn to_json (&self)->OdinServerResult<String> {
        Ok( serde_json::to_string( &self)? )
    }
}

impl <T> Serialize for WsMsg<T> where T: Serialize {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let js_mod_path = format!("{}/{}", self.crate_name, self.js_module);

        let mut state = serializer.serialize_struct("WsMsg", 2)?;
        state.serialize_field("mod", &js_mod_path)?;
        state.serialize_field( &self.payload_name, &self.payload)?;
        state.end()
    }
}

#[macro_export]
macro_rules! ws_msg {
    ($js_module:literal, $p:ident) => {
        odin_server::ws_service::WsMsg::new( env!("CARGO_PKG_NAME"), $js_module, stringify!($p), $p)
    };

    ($crate_name:literal, $js_module:literal, $p:ident) => {
        odin_server::ws_service::WsMsg::new( $crate_name, $js_module, stringify($p), $p)
    }
}

/// syntactic sugar for payload structs we want to send over web sockets
/// This does not provide additional features like the general `define_struct!{..}` macro - it just adds the required serde macros
/// and uses the serde re-export from odin_server so that callers don't have to take care of it
#[macro_export]
macro_rules! define_ws_payload {
    ($svis:vis $sname:ident = $( $fvis:vis $fname:ident : $ftype:ty ),*) => {
        #[derive(serde::Serialize)]
        #[serde(rename_all="camelCase")]
        $svis struct $sname {
            $( $fvis $fname: $ftype,)*
        }
    }
}

/* #endregion WsMsg serialization */