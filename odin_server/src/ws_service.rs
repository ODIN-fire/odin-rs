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
use regex::Match;

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
use std::{any::type_name, sync::LazyLock};
use regex::Regex;

// match js_module_path, payload_name and payload value
static WS_MSG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\{\s*"mod":\s*"([^"]+)"\s*,\s*"([^"]+)":\s*(.*)\}"#).unwrap());

/// wrapper struct for messages sent through the websocket. Each outgoing message is processed by the JS module that
/// has registered for `module_path` with our ws.js JS service module, and each incoming message is dispatched by the
/// SpaServer actor (in `dispatch_incoming_ws_msg()`) to SpaService instances that overload `handle_incoming_ws_msg(..)`
/// Note there is no Deserialize impl for WsMsg since our entry point does not know about T, which is depending on
/// processing service and msg_type
pub struct WsMsg<T>  {
    pub mod_path: &'static str, // this is composed of crate_name/js_module (e.g. "odin_cesium/odin_cesium.js")
    pub msg_type: &'static str, // the operation on the payload
    pub payload: T
}

fn match_str<'a> (s: &'a str, capture: &Match<'a>)-> &'a str {
    &s[capture.start()..capture.len()]
}

/// extrace substrings for module_path, msg_type and payload from incoming JSON string
/// This is our entry-point decoder that just extracts substrings so that SpaServices which process the
/// module_path/msg_type combination can choose respective concrete T types (which have to impl Deserialize)
pub fn extract_ws_msg_parts<'a> (ws_msg: &'a str) -> Option<WsMsgParts<'a>> {
    if let Some(captures) =  WS_MSG_RE.captures(ws_msg) {
        if captures.len() == 4 {
            let m1 = captures.get(1).unwrap();
            let mod_path = match_str( ws_msg, &m1);

            let m2 = captures.get(2).unwrap();
            let msg_type = match_str( ws_msg, &m2);

            let m3 = captures.get(3).unwrap();
            let payload = match_str( ws_msg, &m3);

            return Some( WsMsgParts{ ws_msg, mod_path, msg_type, payload } )
        }
    }

    None
}

/// helper struct that provides str references to the str components of a WsMsg.
/// Used to efficiently dispatch WsMsg sources to the services that process them, without the dispatcher
/// having to know the payload type T
pub struct WsMsgParts<'a> {
    pub ws_msg: &'a str,
    pub mod_path: &'a str,
    pub msg_type: &'a str,
    pub payload: &'a str,
}

impl <T>  WsMsg<T> where T: Serialize {
    pub fn new (mod_path: &'static str, msg_type: &'static str, payload: T)->Self { 
        WsMsg {mod_path, msg_type, payload}
    }

    pub fn to_json (&self)->OdinServerResult<String> {
        Ok( serde_json::to_string( &self)? )
    }

    pub fn json (mod_path: &'static str, msg_type: &'static str, payload: T) -> OdinServerResult<String> {
        Self::new( mod_path, msg_type, payload).to_json()
    }
}

// we need our own Serialize impl since we use the payload_name field as the key for the payload value
impl <T> Serialize for WsMsg<T> where T: Serialize {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("WsMsg", 2)?;
        state.serialize_field("mod", &self.mod_path)?;
        state.serialize_field( &self.msg_type, &self.payload)?;
        state.end()
    }
}

/// note this uses the provided variable name as the msg_type
/// TODO - replace with direct WsMsg::new() calls
#[macro_export]
macro_rules! ws_msg {
    ($mod_path:expr, $payload_var:ident) => {
         WsMsg::new( $mod_path, stringify!($payload_var), $payload_var)
    };
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