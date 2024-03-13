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

#![allow(unused)]

use std::{sync::{Arc, atomic::AtomicU64}};
use futures::{SinkExt,StreamExt,stream::{SplitSink,SplitStream}};
use chrono::Utc;
use tokio_tungstenite::{
    connect_async, WebSocketStream, MaybeTlsStream, 
    tungstenite::{self,
        protocol::Message, 
        http::{Request,header::{AUTHORIZATION,HeaderValue}}, 
        handshake::client::{Response,generate_key}, 
        client::IntoClientRequest
    }
};
use tokio::{net::TcpStream,io::{AsyncRead,AsyncReadExt,AsyncWrite,AsyncWriteExt}};
use reqwest::Client;
use serde::{Deserialize,Serialize};
use serde_json;
use crate::*;


pub type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub async fn init_websocket (config: Arc<SentinelConfig>, device_ids: Vec<String>)->Result<WsStream> {
    let (mut ws_stream,_) = connect( &config).await?;
    expect_connected_response(&mut ws_stream).await?;

    request_join( &mut ws_stream, device_ids, get_next_msg_id()).await?;
    expect_join_response(&mut ws_stream).await?;

    Ok(ws_stream)
}

pub async fn connect (config: &SentinelConfig)->Result<(WsStream, Response)> {
    
    let mut request = config.ws_uri.as_str().into_client_request()?;
    let mut hdrs = request.headers_mut();

    let auth_val = format!("Bearer {}", config.access_token);
    hdrs.append( AUTHORIZATION, HeaderValue::from_str(auth_val.as_str())?);

    /* explicit request construction with Request
    let url = url::Url::parse(&config.ws_uri)?;
    let host = url.host_str().ok_or(op_failed(url::ParseError::EmptyHost))?;

    let request = Request::builder()
        .uri( config.ws_uri.as_str())
        .header("Host", host)
        .header("connection", "Upgrade")
        .header("upgrade", "websocket")
        .header("sec-websocket-version", "13")
        .header("sec-websocket-key", tokio_tungstenite::tungstenite::handshake::client::generate_key())
        .header( "Authorization", format!("Bearer {}", config.access_token))
        .body(())?;
    */

    Ok(connect_async(request).await?)
}

pub async fn expect_connected_response (ws: &mut WsStream)->Result<()> {
    let resp = read_next_ws_msg(ws).await?;
    if let WsMsg::Connected{message} = resp {
        // should we check message == "connected" here?
        Ok(())
    } else {
        Err( OdinSentinelError::WsProtocolError(format!("expected 'connected' message, got {:?}",resp)))
    }
}

pub async fn request_join (ws: &mut WsStream, device_ids: Vec<String>, message_id: String)->Result<()> {
    let msg = WsMsg::Join{device_ids, message_id};
    let json = serde_json::to_string(&msg)?;
    Ok(ws.send( Message::Text(json)).await?)
}

pub async fn expect_join_response (ws: &mut WsStream)->Result<()> {
    let resp = read_next_ws_msg(ws).await?;
    if let WsMsg::Join{device_ids,message_id} = resp  {
        if !device_ids.is_empty() {
            Ok(())
        } else {
            Err( OdinSentinelError::NoDevicesError)
        }
    } else {
        Err( OdinSentinelError::WsProtocolError(format!("expected 'join' message, got {:?}",resp)))
    }
}

pub async fn send_ws_text_msg (tx: &mut SplitSink<WsStream,Message>, msg: String)->Result<()> {
    Ok(tx.send( Message::Text(msg)).await?)
}

pub async fn read_next_ws_msg (ws: &mut WsStream)->Result<WsMsg> {
    let json = ws.next().await.ok_or(tungstenite::error::Error::AlreadyClosed)??;
    let msg: WsMsg = serde_json::from_str( json.to_text()?)?;
    Ok(msg)
}


/* #region websocket messages ***********************************************************************/

// in:      {"event":"connected","data": {"message": "connected"}}
// out+in:  {"event":"join", "data":{ "deviceIds":["roo7gd1dldn3"], "messageId":"test-1"}}
// in:      {"event":"record","data":{"deviceId":"roo7gd1dldn3","sensorNo":37,"type":"image"}}

/// the notifications we get from the Delphire server through the websocket
#[derive(Serialize,Deserialize,Debug,PartialEq)]
#[serde(tag="event", content="data", rename_all="lowercase", rename_all_fields="camelCase")]
pub enum WsMsg {
    Connected { message: String },

    Join { device_ids: Vec<String>, message_id: String },

    Record { device_id: String, sensor_no: u32, #[serde(alias="type")] rec_type: SensorCapability },

    Pong { request_time: u64, response_time: u64, message_id: String },

    #[serde(alias="trigger-alert")] 
    TriggerAlert { device_id: String, message_id: String, result: String },

    Error { message: String }
}

/// outgoing websocket messages
#[derive(Serialize,Deserialize,Debug,PartialEq)]
#[serde(tag="event", content="data", rename_all="lowercase", rename_all_fields="camelCase")]
pub enum WsCmd {
    Ping { request_time: u64, message_id: String },  // time is epoch millis

    #[serde(alias="trigger-alert")] 
    TriggerAlert { device_ids: Vec<String>, message_id: String },

    #[serde(alias="switch-lights")] 
    SwitchLights { device_ids: Vec<String>, #[serde(alias="type")] light_type: String, state: String, message_id: String },

    #[serde(alias="switch-valve")]
    SwitchValve { device_ids: Vec<String>, state: String, message_id: String  },
}

impl WsCmd {
    pub fn new_ping (msg_id: impl ToString)-> WsCmd {
        WsCmd::Ping { request_time: Utc::now().timestamp_millis() as u64, message_id: msg_id.to_string() }
    }
}

/* #endregion websocket messages */