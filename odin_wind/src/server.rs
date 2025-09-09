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

use std::{collections::{HashMap,HashSet}, default::Default, error::Error, net::{IpAddr, SocketAddr}, path::{Path,PathBuf}, sync::{Arc,Mutex}, time::SystemTime};

use axum::{
    body::Body, debug_handler, 
    extract::{self, connect_info::ConnectInfo, ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade}, MatchedPath, Query, Path as AxumPath}, 
    http::{Request, StatusCode as AxStatusCode}, response::{Html, IntoResponse, Response}, 
    routing::{get,post}, 
    Json, Router
};
use http::StatusCode;
use serde::{Serialize,Deserialize};
use serde_json;
use futures_util::{sink::SinkExt, stream::{StreamExt, SplitSink, SplitStream}};
use tokio::{net::TcpListener, task::AbortHandle};
use tracing_subscriber::EnvFilter;
use odin_build::set_bin_context;
use odin_common::{
    collections::process_async, datetime::secs, 
    ron::{TypedCompactRon,from_typed_compact_ron},
    fs::EMPTY_PATH,
    ws::{WsConnection,AddWsConnection,RemoveWsConnection}
};
use odin_actor::prelude::*;
use odin_server::{spawn_server_task, ServerConfig};
use crate::{
    actor::WindActorMsg, 
    errors::{op_failed, Result}, 
    SubscribeResponse, AddWindClient, AddWindClientResponse, RemoveWindClient, RemoveWindClientResponse, WindConfig, WindRegion, Forecast, PKG_CACHE_DIR
};


struct WindRegionEntry {
    region: WindRegion,
    clients: HashSet<Arc<WsConnection>>,
}

pub struct WindServer {
    config: ServerConfig,
    name: String,
    hwind: ActorHandle<WindActorMsg>,

    // keeping subscriptions separate requires notification overhead (connections lookup) but safely separates connection from subscription
    // since notifications are not frequent (minute range) the overhead should not be relevant
    regions: HashMap<String,HashSet<SocketAddr>>, // region-name -> [connections]
    connections: HashMap<SocketAddr,WsConnection>,

    server_task: Option<JoinHandle<()>>
}

impl WindServer {

    pub fn new (config: ServerConfig, name: impl ToString, hwind: ActorHandle<WindActorMsg>)->Self {
        let regions = HashMap::new();
        let name = name.to_string();
        let connections = HashMap::new();

        WindServer { config, name, hwind, regions, connections, server_task: None }
    }

    fn start_server (&mut self, hself: ActorHandle<WindServerMsg>)->Result<()> {
        if self.server_task.is_none() {
            if cfg!(feature="trace_server") {
                // note this only succeeds if there is no global subscriber set yet
                tracing_subscriber::fmt()
                    .with_env_filter(EnvFilter::from_default_env())  // use RUST_LOG to set max level
                    //.with_max_level(tracing::Level::DEBUG)
                    .try_init();
            }
        }

        println!("serving wind data on {}/{}", self.config.url(), self.name);
        let router = self.build_router( &hself);
        self.server_task = Some(spawn_server_task( &self.config, router));

        Ok(())
    }

    fn build_router (&self, hself: &ActorHandle<WindServerMsg>)->Router {
        Router::new()
            .route( &format!("/{}/ws", self.name.as_str()), get( {
                let hself = hself.clone();
                move |ws: WebSocketUpgrade, ci: ConnectInfo<SocketAddr>| { Self::ws_handler(ws, ci, hself) }
            }))
            .route( &format!("/{}/wind-data/{{*unmatched}}", self.name.as_str()), get( Self::data_handler)) // serve the WindNinja output
    }

    async fn ws_handler (ws: WebSocketUpgrade, ConnectInfo(addr): ConnectInfo<SocketAddr>, hself: ActorHandle<WindServerMsg>)->Response {
        ws.on_upgrade( move |socket| Self::handle_socket(socket, addr, hself))
    }

    async fn data_handler (path: AxumPath<String>) -> Response {
        // this is served from our cache dir as compressed CSV or JSON files
        odin_server::compressable_file_response::<&Path>( PKG_CACHE_DIR.as_ref(), path.as_str(), "windninja data not found")
    }

    async fn handle_socket(mut ws: WebSocket, remote_addr: SocketAddr, hself: ActorHandle<WindServerMsg>) {
        hself.send_msg( AddWsConnection{remote_addr,ws}).await;
    }

    async fn add_connection(&mut self, hself: ActorHandle<WindServerMsg>, remote_addr: SocketAddr, ws: WebSocket)->Result<()> {
        if !self.connections.contains_key( &remote_addr) {
            let name = remote_addr.to_string();
            let (mut ws_sender, mut ws_receiver) = ws.split();

            let ws_receiver_task = {
                let hself = hself.clone();
                let remote_addr = remote_addr.clone();

                spawn( &name, async move {
                    while let Some(Ok(msg)) = ws_receiver.next().await {
                        match msg.into_text() {
                            Ok(msg) => {
                                if !msg.is_empty() {
                                    let msg = msg.to_string();
                                    hself.send_msg( ProcessIncomingWsMsg{msg, remote_addr}).await;
                                }
                            }
                            Err(e) => println!("ignoring binary message")
                        }
                    }

                    // connections closed - clean up
                    hself.send_msg( RemoveWsConnection{remote_addr}).await;
                })?
            };

            info!("connected to client {}", remote_addr);
            self.connections.insert( remote_addr.clone(), WsConnection{ remote_addr, ws_sender, ws_receiver_task });
        }

        Ok(())
    }

    async fn remove_connection(&mut self, remote_addr: SocketAddr)->Result<()> {
        // clean up - we get this from a dropped websocket
        info!("server removing connection {} due to dropped websocket", remote_addr);

        for (_,clients) in self.regions.iter_mut() { clients.remove( &remote_addr); }
        self.connections.remove(&remote_addr);

        self.hwind.send_msg( RemoveWindClient{region: None, remote_addr}).await?;

        Ok(())
    }

    /// note this is sent from an Odin WindServerClient and uses typed compact RON
    async fn process_incoming_ws_msg (&mut self, msg: String, remote_addr: SocketAddr)->Result<()> {
        if let Some(mut msg) = from_typed_compact_ron::<AddWindClient>( &msg) {
            msg.remote_addr = remote_addr;
            return Ok( self.hwind.send_msg( msg).await? ) // this will cause us to receive a SubscribeResponse
        }

        if let Some(mut msg) = from_typed_compact_ron::<RemoveWindClient>( &msg) {
            info!("client {} unsubscribed {:?}", remote_addr, msg.region);

            // internal housekeeping
            if let Some(region) = &msg.region { // remove only for this region
                if let Some(clients) = self.regions.get_mut( region) { clients.remove( &remote_addr); }
            } else { // drop all regions for this client
                for (_,clients) in self.regions.iter_mut() { clients.remove( &remote_addr); }
            }

            // now forward to our WindActor
            msg.remote_addr = remote_addr; // fill in the real remote addr (which we get from our websocket conn)
            return Ok( self.hwind.send_msg( msg).await? ) // this is fire-and-forget (no reply)
        }

        Ok(()) // TODO - really Ok if we can't deserialize the message ?
    }

    /// this processes subscribe responses from our WindActor - the remote addr is the connected SpaServer (not a browser)
    async fn process_subscribe_response (&mut self, response: SubscribeResponse)->Result<()> {
        match response {
            SubscribeResponse::Add( response ) => {
                if let Some(remote_addr) = &response.remote_addr {
                    if let Some(conn) = self.connections.get_mut( remote_addr) { // if we don't have a connection there is nothing we can do
                        if response.rejection.is_none() { // subscription was successful
                            if let Some(clients) = self.regions.get_mut( &response.wn_region.name) { // we alreay monitor this region
                                clients.insert( remote_addr.clone());
                            } else { // new region
                                self.regions.insert( response.wn_region.name.clone(), HashSet::from([remote_addr.clone()]));
                            }
                        }

                        send_to_ws_connection(conn, response).await;
                    }
                }
            }
            SubscribeResponse::Remove( remove_client_response ) => {

            }
        }
        Ok(())
    }

    async fn process_forecast (&mut self, mut forecast: Forecast)->Result<()> {
        if let Some(clients) = self.regions.get( forecast.region.as_str()) {
            // zero out out server paths - we shouldn't expose them to clients
            forecast.wx_path = EMPTY_PATH.clone(); 
            forecast.dem_path = EMPTY_PATH.clone();

            for remote_addr in clients {
                if let Some(conn) = self.connections.get_mut( remote_addr) {
                    send_to_ws_connection(conn, forecast.clone()).await
                }
            }

            //process_async( clients.iter(), forecast, async |remote_addr,fc| {
            //    if let Some(conn) = self.connections.get_mut( remote_addr) {
            //        send_to_ws_connection(conn, fc).await;
            //    }
            //}).await;
        }

        Ok(())
    }

    fn stop_server (&mut self)->Result<()> {
        if let Some(jh) = &self.server_task {
            jh.abort();
            self.server_task = None;
            Ok(())
        } else {
            Err(op_failed("server task not running"))
        }
    }
}

async fn send_to_ws_connection<'a,T> (conn: &mut WsConnection, msg: T) where T: TypedCompactRon<'a> {
    if let Ok(ron) = msg.to_typed_compact_ron() {
        conn.ws_sender.send( Message::text(ron)).await; 
    }
}

define_actor_msg_set! { pub WindServerMsg =
    AddWsConnection | RemoveWsConnection | ProcessIncomingWsMsg | SubscribeResponse | Forecast
}



/// process message that was received through the web socket
#[derive(Debug)]
pub struct ProcessIncomingWsMsg {
    msg: String,
    remote_addr: SocketAddr
}

impl_actor! { match actor_msg for Actor<WindServer,WindServerMsg> as
    _Start_ => cont! {
        let hself = self.hself.clone();
        if let Err(e) = self.start_server( hself) {
            error!("failed to start server: {e:?}");
        }
    }

    AddWsConnection => cont! {
        let hself = self.hself.clone();
        self.add_connection( hself, actor_msg.remote_addr, actor_msg.ws).await;
    }

    RemoveWsConnection => cont! {
        self.remove_connection( actor_msg.remote_addr).await;
    }

    ProcessIncomingWsMsg => cont! {
        self.process_incoming_ws_msg( actor_msg.msg, actor_msg.remote_addr).await;
    }

    SubscribeResponse => cont! {
        self.process_subscribe_response(actor_msg).await;
    }

    Forecast => cont! {
        self.process_forecast( actor_msg).await;
    }

    _Terminate_ => stop! {
        self.stop_server();
    }
}

// subscribe action for a WindServer 
pub fn wind_server_subscribe_action (hserver: ActorHandle<WindServerMsg>) -> impl DataAction<SubscribeResponse> {
    data_action!( let hserver: ActorHandle<WindServerMsg> = hserver.clone() => |response: SubscribeResponse| {
        hserver.send_msg(response).await;
        Ok(())
    })
}

/// standard update action that broadcasts `forecast` websocket messages through a SpaServer
pub fn wind_server_update_action (hserver: ActorHandle<WindServerMsg>) -> impl DataRefAction<Forecast> {
    dataref_action!( let hserver: ActorHandle<WindServerMsg> = hserver.clone() => |forecast: &Forecast| {  // update action
        hserver.send_msg( forecast.clone()).await;
        Ok(())
    })
}