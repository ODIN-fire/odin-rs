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

use std::{sync::Arc,result::Result,error::Error, time::Duration, net::SocketAddr};
use serde::{Serialize,Deserialize};
use futures_util::{sink::SinkExt, stream::{StreamExt, SplitSink, SplitStream}};
use tokio::{select,net::TcpStream,io::{AsyncRead,AsyncReadExt,AsyncWrite,AsyncWriteExt}, time::{sleep,Sleep}, task::JoinHandle};
use tokio_tungstenite::{
    connect_async, WebSocketStream, MaybeTlsStream, 
    tungstenite::{self,
        protocol::Message, 
        http::{Request,header::{AUTHORIZATION,HeaderValue}}, 
        handshake::client::{Response,generate_key}, 
        client::IntoClientRequest
    }
};
use axum::extract::ws::{WebSocket,Message as AxMessage};
use kanal::AsyncReceiver; // TODO - should we make this abstract?
use crate::{datetime::secs, net::OdinNetError, auth::{ConnUser,MsgAcl,RoleSet}};

pub type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// struct to keep track of active (bidirectional) websocket connections
pub struct WsConnection {
    pub remote_addr: SocketAddr,
    pub ws_sender: SplitSink<WebSocket,AxMessage>, // used to send through the websocket
    pub ws_receiver_task: JoinHandle<()>, // the task that (async) reads from the websocket
    pub user: Option<Arc<ConnUser>>, // authenticated user (None if the server does not use authentication)
    pub out_acl: Option<Arc<MsgAcl>>, // role requirements for outgoing messages (None if there are none)
}

impl WsConnection {
    /// send a websocket message if the user of this connection is authorized for it (see [`Self::may_receive`]).
    /// Messages for which the user is not authorized are silently dropped (this is not an error).
    /// Note this should not be used if we send multiple messages to the same connection (use feed() or send_all() in this case)
    pub async fn send (&mut self, msg: String)->Result<(),OdinNetError> {
        if !self.may_receive( &msg) { return Ok(()) }
        self.ws_sender.send( AxMessage::text(msg)).await.map_err(|e| OdinNetError::WsError( e.to_string()))
    }

    /// is the user of this connection authorized to receive `msg`. This is always true if there is no user (server
    /// without authentication) or no outgoing message ACL. Only messages created by `WsMsg` (`{"mod":..,"<msg_type>":..}`)
    /// can be filtered - free-form messages are always sent
    #[inline]
    pub fn may_receive (&self, msg: &str)->bool {
        match (&self.user, &self.out_acl) {
            (Some(user), Some(acl)) => acl.required_for_ws_msg( msg).map( |r| user.roles.satisfies(r)).unwrap_or(true),
            _ => true
        }
    }

    /// is the user of this connection authorized for messages that require `required` roles (`None` means not protected).
    /// Use this if the same message is sent to several connections, to avoid re-computing the required roles
    #[inline]
    pub fn is_authorized_for (&self, required: Option<RoleSet>)->bool {
        match (required, &self.user) {
            (Some(r), Some(user)) => user.roles.satisfies(r),
            _ => true
        }
    }

    /// name of the authenticated user (None if the server does not use authentication)
    pub fn user_name (&self)->Option<&str> {
        self.user.as_ref().map( |u| u.name.as_ref())
    }

    /// is this connection authorized for `required` roles (always true if the server does not use authentication)
    pub fn is_authorized (&self, required: RoleSet)->bool {
        crate::auth::is_authorized( self.user.as_deref(), required)
    }
}

/// message type to process new connections
#[derive(Debug)]
pub struct AddWsConnection {
    pub remote_addr: SocketAddr,
    pub ws: WebSocket,
    pub user: Option<Arc<ConnUser>>, // authenticated user of the websocket upgrade request (None if no authentication)
}

/// message type to remove connections
#[derive(Debug)]
pub struct RemoveWsConnection {
    pub remote_addr: SocketAddr,
}

pub async fn ws_loop<E: Error> (ws_uri: String, access_token: String, ws_rx: AsyncReceiver<String>, reconnect_delay: Option<Duration>, proc_incoming: impl AsyncFn(Message)->Result<(),E>) {
    loop {
        // TODO - report re-init
        match connect( ws_uri.as_str(), access_token.as_str()).await {
            Ok((mut ws_stream,_)) => {
                loop {
                    select! { // NOTE - this requires all awaited futures to be cancellation safe !
                        maybe_msg = ws_stream.next() => { // in: notification from server
                            match maybe_msg {
                                Some(msg) => match msg {
                                    Ok(msg) => {
                                        if let Err(e) = proc_incoming(msg).await {
                                            eprintln!("processing of incoming msg failed: {}", e)
                                        };
                                    }
                                    Err(e) => {
                                        eprintln!("reconnecting after failed websocket read: {}", e);
                                        break; // do we have to check the tungstenite::error::Error variant? I seems they all warrant restart
                                    }
                                }
                                None => { 
                                    eprintln!("server closed websocket, trying to reconnect..");
                                    break; // try to re-connect
                                }
                            }
                        }

                        maybe_req = ws_rx.recv() => { // out: request to server
                            match maybe_req {
                                Ok(req) => {
                                    if let Err(e) = ws_stream.send( Message::text(req)).await {
                                        eprintln!("failed to write to websocket: {}", e);
                                        break; // try to re-connect
                                    }
                                }
                                Err(e) => {
                                    // cmd queue closed - terminate (this is nominal termination, no error)
                                    return
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("{e}");
            }
        }
        if let Some(dur) = reconnect_delay {
            sleep(dur).await;
        }
    }
}

pub async fn connect (ws_uri: &str, access_token: &str)->Result<(WsStream, Response),OdinNetError> {
    let mut request = ws_uri.into_client_request().map_err(|e| OdinNetError::OpFailed(format!("invalid websocket URL: {e}")))?;

    let mut hdrs = request.headers_mut();
    let auth_val = format!("Bearer {}", access_token);
    hdrs.append( AUTHORIZATION, HeaderValue::from_str(auth_val.as_str()).map_err(|e| OdinNetError::OpFailed(format!("invalid auth header: {e}")))?);

    //let request = Request::builder()
    //    .uri( ws_uri)
    //    .header("Host", "localhost:9010")
    //    //.header("connection", "Upgrade")
    //    .header("connection", "keep-alive,Upgrade") // ? doesn't keep alive
    //    .header("upgrade", "websocket")
    //    .header("sec-websocket-version", "13")
    //    .header("sec-websocket-key", tokio_tungstenite::tungstenite::handshake::client::generate_key())
    //    //.header( "Authorization", format!("Bearer {}", config.access_token))
    //    .body(()).map_err(|e| OdinNetError::WsError(e.to_string()))?;


    match connect_async(request).await {
        Ok(ws_stream) => {
            Ok( ws_stream)
        }
        Err(e) => {
            Err(OdinNetError::OpFailed(format!("websocket connect failed: {e}")))
        }
    }
}