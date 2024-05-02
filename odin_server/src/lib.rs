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
//#![feature(diagnostic_namespace)]

use std::{ any::Any, 
    fmt::{Debug,Write}, 
    net::{IpAddr, SocketAddr}, 
    path::Path, 
    collections::{HashSet,HashMap}, 
    convert::Infallible, 
    future::{Future,ready},
    sync::Arc,
};
//use axum_macros::*;
use axum::{
    http::Uri,
    routing::get, Router,
    serve::IncomingStream,
    extract::{
        Path as AxumPath, Query, Request, State,
        connect_info::ConnectInfo,
        ws::{WebSocket,Message,WebSocketUpgrade}
    },
    response::{IntoResponse,Response,Html},
};
use axum_extra::TypedHeader;
use headers;
use tower::ServiceExt;
use tower_http::{services::{ServeDir, ServeFile},trace::TraceLayer};
use futures_util::{sink::SinkExt, stream::{StreamExt, SplitSink, SplitStream}};
use tokio::{net::ToSocketAddrs};
use odin_actor::{prelude::*, errors::{Result,op_failed}};
use odin_actor::tokio_kanal::{Actor,ActorHandle,AbortHandle,Query as ActorQuery,JoinHandle, query, spawn};

pub mod imagery_service;

/// trait that defines the micro service interface. Note that this trait is not object safe since it uses RPITIT, hence
/// we cannot use Box<dyn MicroService> to store collections of it. Use the [`odin_macros::define_service_type`]` macro to define a type
/// that preserves the concrete types and provides a MicroService impl for the collection
pub trait MicroService: Send + 'static {
    /// return optional router for this service (default is None). This is called during _Start_ initialization of the server actor
    fn router (&self, hserver: ActorHandle<ServerMsg>)->Option<Router> { None }

    /// this is an optional async function that can send initial data (e.g. state snapshots) once a Websocket request got upgraded
    fn send_init_ws_msg (&self, hserver: ActorHandle<ServerMsg>, remote_addr: SocketAddr)->impl Future<Output=Result<()>> + Send { ready(Ok()) }

    /// this is an optional async function that is called upon receiving a Websocket message from the client in the websocket input task
    fn handle_incoming_ws_msg (&self, hserver: ActorHandle<ServerMsg>, msg: &str)->impl Future<Output=Result<()>> + Send { ready(Ok()) }
}




/* #region server **********************************************************************************************/

/// object to describe role of a user
pub struct UserRole {
    // TBD
}

pub struct Server<S> where S: MicroService {
    app_name: String,
    addr_spec: String,
    doc_path: String,

    services: Arc<S>,
    connections: HashMap<SocketAddr,Connection>,
    server_task: Option<JoinHandle<()>> // for the server task itself
}

struct Connection {
    user_role: UserRole,
    remote_addr: SocketAddr,
    ws_sender: SplitSink<WebSocket,Message>, // used to send through the websocket
    ws_receiver_task: JoinHandle<()> // the task that (async) reads from the websocket
}

impl<S> Server<S> where S: MicroService {
    pub fn new (app_name: impl ToString, addr_spec: impl ToString, doc_path: impl ToString, services: S)->Self {
        Server { 
            app_name: app_name.to_string(),
            addr_spec: addr_spec.to_string(), 
            doc_path: doc_path.to_string(),

            services: Arc::new(services), 
            connections: HashMap::new(), 
            server_task: None
        }
    }

    // this is called from _Start_
    fn start_server (&mut self, hself: ActorHandle<ServerMsg>)->Result<()> {
        if self.server_task.is_none() {
            let addr_spec = self.addr_spec.clone();
            let router = self.collect_routes( &hself)
                .into_make_service_with_connect_info::<SocketAddr>();

            self.server_task = Some( tokio::spawn( async move {
                let listener = tokio::net::TcpListener::bind(addr_spec).await.unwrap();
                axum::serve( listener, router).await.unwrap();    
            }));
            Ok(())
        } else {
            Err(op_failed("server task already running"))
        }
    }

    // this is called from _Terminate_
    fn stop_server (&mut self)->Result<()> {
        if let Some(jh) = self.server_task {
            jh.abort();
            self.server_task = None;
            Ok(())
        } else {
            Err(op_failed("server task not running"))
        }
    }

    fn collect_routes  (&self, hself: &ActorHandle<ServerMsg>)->Router {
        let mut router = Router::new()
            .route( &self.doc_path.as_str(), get( doc_handler))
            .with_state(hself.clone());
            //.into_make_service_with_connect_info::<SocketAddr>();

        if let Some(svc_router) = self.services.router(hself.clone()) {
            router = router.merge( svc_router);
        }

        router
    }

    async fn add_connection(&mut self, hself: ActorHandle<ServerMsg>, remote_addr: SocketAddr, ws: WebSocket) {
        let raddr = remote_addr.clone();
        let (mut ws_sender, mut ws_receiver) = ws.split();

        let ws_receiver_task = spawn( async move {
            while let Some(Ok(msg)) = ws_receiver.next().await {
                // TBD
            }
        });

        self.push_initial_msgs( &mut ws_sender).await;

        let conn = Connection { remote_addr, ws_sender, ws_receiver_task };
        self.connections.insert( raddr, conn);
    }

    fn get_user_role (&self)->Option<UserRole> {
        Some(UserRole{}) // just a placeholder for now
    }

    async fn push_initial_msgs (&mut self, ws_sender: &mut SplitSink<WebSocket,Message>) {
        // this either send PushWsMsg to the server if the services own the data (e.g. for completely configured data)
        // or sends SendSnapshot messages to respective updater actors, which reply by sending the snapshot data directly
        // to the Server, which then sends it to the single connection for which it was requested
    }

    async fn push_msg_to_all (&mut self, m: String) {
        // TODO - use feed() or send_all() for batches
        let ws_msg = Message::Text(m);
        for conn in self.connections.values_mut() {
            conn.ws_sender.send(ws_msg.clone()).await; 
        }
    }

    async fn push_msg_to_connection (&mut self, remote_addr: SocketAddr, m: String) {
        if let Some(conn) = self.connections.get_mut( &remote_addr) {
            conn.ws_sender.send(Message::Text(m)).await;
        }
    }

}

/// the trait that abstracts authenticating users, determining UserRole for authenticated users and then selecting 
/// document variants based on role, user_agent and connect info
trait UserPolicy {
    fn get_user_role (&self, remote_addr: SocketAddr, host: Option<String>)->Option<UserRole>; // TODO - this might need more input for authentication
    fn get_document_path (&self, role: UserRole, remote_addr: SocketAddr, host: Option<String>, user_agent: Option<String>)->Option<PathBuf>;
}

/// we don't use the general Tower ServeDir service here since we want to be able to use user-agent, host and connect info
/// to select specific document variants, e.g. to adapt CSS or to restrict services based on user role/permission 
//#[debug_handler].  // constraint: debug_handler macro does not work for impl block methods without self arg
async fn doc_handler (
    State(user_policy): State<Arc<UserPolicy>>,
    user_agent: Option<TypedHeader<headers::UserAgent>>,
    host: Option<TypedHeader<headers::Host>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>
) -> Html<String>
{
    
}

/* #endregion server */

/* #region server actor **************************************************************************************/

#[derive(Debug)]
pub struct AddConnection { remote_addr: SocketAddr, ws: WebSocket }

#[derive(Debug)]
pub struct PushWsMsgToAll { 
    pub data: String 
}

#[derive(Debug)]
pub struct PushWsMsgToConnection { 
    pub remote_addr: SocketAddr, 
    pub data: String 
}

define_actor_msg_set! { ServerMsg = 
    AddConnection | 
    PushWsMsgToAll |
    PushWsMsgToConnection
}

impl_actor! { match msg for Actor<Server<S>,ServerMsg> where S:MicroService as
    _Start_ => cont! { 
        let hself = self.hself.clone();
        self.start_server( hself)
    }
    AddConnection => cont! {
        let hself = self.hself.clone();
        self.add_connection( hself, msg.remote_addr, msg.ws).await;
    }
    PushWsMsgToAll => cont! {
        self.push_msg_to_all( msg.data).await;
    }
    PushWsMsgToConnection => cont! {
        self.push_msg_to_connection( msg.remote_addr, msg.data).await;
    }
    _Terminate_ => stop! {
        self.stop_server();
    }
}

/* #endregion server actor */
