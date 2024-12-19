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

use std::{boxed, collections::HashMap, sync::Arc, ops::{Deref,DerefMut}, 
    net::SocketAddr, future::{Future,ready}, time::SystemTime,
    path::{PathBuf}, any::type_name, fmt::Write,
    result::Result, error::Error
};
use axum::{
    body::Body,
    extract::{
        connect_info::ConnectInfo,
        ws::{Message, WebSocket, WebSocketUpgrade},
        FromRef, Path as AxumPath, Query, RawQuery, Request, State
    },
    http::{HeaderMap, StatusCode, Uri},
    middleware::map_request, response::{Html, IntoResponse, Response},
    routing::get,
    Router,ServiceExt
};
use axum_server::{service::MakeService, tls_rustls::RustlsConfig};
use bytes::Bytes;
use futures_util::{sink::SinkExt, stream::{StreamExt, SplitSink, SplitStream}};
use http_body::Body as _;
use http_body_util::{Full, BodyExt, combinators::UnsyncBoxBody};
use odin_build::OdinBuildError;
use tower_http::{services::ServeDir,trace::TraceLayer};
use tracing_subscriber::EnvFilter;
use reqwest::{header::{self, SET_COOKIE}, Client, RequestBuilder};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, RequestBuilder as MwRequestBuilder};
use http_cache_reqwest::{Cache, CacheMode, CACacheManager, HttpCache, HttpCacheOptions};
use serde::{Deserialize,Serialize};
use async_trait::async_trait;

use odin_build::LoadAssetFp;
use odin_common::{fs::get_file_basename,strings::{self, mk_query_string}};
use odin_macro::define_struct;
use odin_actor::prelude::*;

use crate::{load_asset, asset_uri, self_crate, get_asset_response, spawn_server_task, ServerConfig, WsMsg, WsMsgParts, ws_service};
use crate::errors::{connect_error, init_error, op_failed, OdinServerError, OdinServerResult};

/// the trait that abstracts a single page application service, which normally represents a visualization
/// layer with its own data (either dynamic or static) and document assets (such as Javascript modules
/// and images) or fragments (HTML elements)
#[async_trait]
pub trait SpaService: Send + Sync + 'static {
    /// override this if the service depends on other services. Default is it doesn't
    fn add_dependencies (&self, sb: SpaServiceList)->SpaServiceList {sb} // defaut is no dependencies

    /// this adds document fragments and route data for this micro service
    /// Called during server construction to accumulate components of all included SpaServices
    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>;

    /// is this a service that implements a websocket
    fn is_websocket (&self)->bool {
        false
    }

    /// called from server actor after receiving an AddConnection message from the ws route handler
    /// If data is not owned by service this triggers a data action
    /// NOTE: this is called from within the actor loop of the server, i.e. we should NOT await message sends
    /// to the server from within init_connection() implementations as this might deadlock if the server mailbox is full.
    /// Directly send websocket messages through `conn.send(..)` in this case (which is also more efficient)
    async fn init_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut SpaConnection) -> OdinServerResult<()> {
        Ok(())
    }

    /// can be used to broadcast newly available data to all connections. This is normally sent from an init action
    /// of the actor providing service data. It is useful if the ws messages to be sent would be expensive to create
    /// and hence should be avoided if there are no connections yet.
    /// Returns a result with a boolean value indicating if the data required by the service is available
    async fn data_available (&mut self, hself: &ActorHandle<SpaServerMsg>, has_connections: bool,
                             sender_id: &str, data_type: &str) -> OdinServerResult<bool> {
        Ok(true)
    }

    /// called from within the server task. Override if service processes incomingg websocket message.
    /// Although we pass in hself and hence services could send SendWsMsg/BroadcastWsMsg messages to respond we also
    /// use a result type that can bypass additional messages since this is already executing in the SpaServer actor task
    async fn handle_ws_msg (&mut self, 
        hself: &ActorHandle<SpaServerMsg>, remote_addr: &SocketAddr, ws_msg_parts: &WsMsgParts
    ) -> OdinServerResult<WsMsgReaction> {
        Ok( WsMsgReaction::None )
    }
}

/// Service response to incoming websocket messages
#[derive(PartialEq)]
pub enum WsMsgReaction {
    Send(String),
    Broadcast(String),
    None
}

/// SpaServer internal structure to keep track of SpaService objects and their server-specific state
struct SpaSvc {
    service: Box<dyn SpaService>, // we can keep this in a Box since this is not shared and only used from within the actor task
    is_data_available: bool, // this is where we store the data_available() response of the service
}

impl SpaSvc {
    pub fn new (service: impl SpaService)->Self {
        SpaSvc {
            service: Box::new(service),
            is_data_available: false,
        }
    }
}

impl Deref for SpaSvc {
    type Target = Box<dyn SpaService>;
    fn deref(&self) -> &Self::Target { &self.service }
}

impl DerefMut for SpaSvc {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.service }
}

/// an object to build SpaService lists from services that can recursively depend on other services.
/// Each service type is included just once, in the order of first occurrence
pub struct SpaServiceList {
    seen: Vec<&'static str>,
    services: Vec<SpaSvc>,
}

impl SpaServiceList {
    pub fn new ()->Self { SpaServiceList{seen: Vec::new(), services: Vec::new()} }

    pub fn add<F,T> (self, svc_ctor: F)->Self where F: FnOnce()->T, T: SpaService + 'static {
        let name = type_name::<T>();
        if !self.seen.contains(&name) {
            let svc = svc_ctor();
            let mut sb = svc.add_dependencies( self);
            sb.seen.push(name);

            let svc_state = SpaSvc::new(svc);
            sb.services.push( svc_state);

            sb
        } else {
            self
        }
    }
}

/// struct to keep track of active SinglePageApp connections
pub struct SpaConnection {
    pub remote_addr: SocketAddr,
    pub ws_sender: SplitSink<WebSocket,Message>, // used to send through the websocket
    pub ws_receiver_task: JoinHandle<()> // the task that (async) reads from the websocket
}

impl SpaConnection {
    // note this should not be used if we send multiple messages to the same connection (use feed() or send_all() in this case)
    pub async fn send (&mut self, msg: String)->OdinServerResult<()> {
        Ok( self.ws_sender.send( Message::Text(msg)).await? )
    }
}

/// this is the state that can be passed into service specific axum handlers
/// note this has to clone efficiently and needs to be invariant
#[derive(Clone)]
pub struct SpaServerState {
    pub name: Arc<String>,
    pub hself: ActorHandle<SpaServerMsg>
}

/// the actor state for a single page application server actor
pub struct SpaServer {
    config: ServerConfig,
    name: String, // this is not from the config so that we can have the same for different apps
    services: Vec<SpaSvc>,

    connections: HashMap<SocketAddr,SpaConnection>, // updated when receiving an AddConnection actor message
    server_task: Option<JoinHandle<()>>, // for the server task itself, initialized upon _Start_
}

impl SpaServer {

    pub fn new (config: ServerConfig, name: impl ToString, service_list: SpaServiceList)->Self {
        SpaServer {
            config,
            name: name.to_string(),
            services: service_list.services,
            connections: HashMap::new(),
            server_task: None,
        }
    }

    fn requires_websocket (&self)->bool {
        self.services.iter().find( |s| s.is_websocket()).is_some()
    }

    fn has_connections (&self)->bool {
        !self.connections.is_empty()
    }

    /// called when receiving _Start_ message
    fn start_server (&mut self, hself: ActorHandle<SpaServerMsg>)->OdinServerResult<()> {
        if self.server_task.is_none() {
            if cfg!(feature="trace_server") {
                // note this only succeeds if there is no global subscriber set yet
                tracing_subscriber::fmt()
                    .with_env_filter(EnvFilter::from_default_env())  // use RUST_LOG to set max level
                    //.with_max_level(tracing::Level::DEBUG)
                    .try_init();
            }

            println!("serving SPA on {}/{}", self.config.url(), self.name);
            let router = self.build_router( &hself)?;
            self.server_task = Some(spawn_server_task( &self.config, router));
            Ok(())

        } else {
            Err(op_failed("server task already running"))
        }
    }

    fn build_router (&self, hself: &ActorHandle<SpaServerMsg>)->OdinServerResult<Router> {
        let comps = SpaComponents::from_svcs( &self.services)?;
        let doc = Arc::new(comps.to_html( &self.name));
        let proxies = comps.proxies;
        let assets = comps.assets;

        let mut router = Router::new()
            //--- the document route
            .route( &format!("/{}", self.name), get({
                let doc = doc.clone();
                move |req: Request| { Self::doc_handler( req, doc) }
            }));

        // add service specific routes
        if !comps.routes.is_empty() {
            let spa_server_state = SpaServerState { // note this is immutable state
                name: Arc::new( self.name.clone()),
                hself: hself.clone(),
            };
            for rf in comps.routes {
                router = rf(router, spa_server_state.clone());
            }
        }

        // now add the generic routes for proxies and assets
        router = router
            .route( &format!("/{}/proxy/*unmatched", self.name), get({
                let mode = CacheMode::Default;
                let manager = CACacheManager { path: odin_build::cache_dir().join("proxies") };
                let options = HttpCacheOptions::default();
                let http_client = ClientBuilder::new(Client::new())
                    .with( Cache( HttpCache {mode, manager, options}))
                    .build();
                //move |uri_elems: Path<(String,String)>, req: Request| { Self::proxy_handler(uri_elems, req, http_client, proxies) }
                move |path: AxumPath<String>, query: RawQuery, req: Request| { Self::proxy_handler(path, query, req, http_client, proxies) }
            }))

            // 'key' is the owning crate
            .route( &format!("/{}/asset/:key/*unmatched", self.name), get({
                move |uri_elems: AxumPath<(String,String)>, req: Request| { Self::asset_handler(uri_elems, req, assets)}
            }));

        // note this won't do anything unless there also is a tracing subscriber set somewhere
        if cfg!(feature="trace_server") {
            router = router.layer(TraceLayer::new_for_http());
        }

        Ok(router)
    }

    async fn doc_handler (req: Request, doc: Arc<String>) -> Response {
        (StatusCode::OK, Body::from(doc.to_string())).into_response()
    }

    async fn proxy_handler (path: AxumPath<String>, query: RawQuery, req: Request,
                            http_client: ClientWithMiddleware, proxies: HashMap<String,ProxySpec>) -> Response {
        if let Some(idx) = path.find('/') {
            let key = &path[0..idx];

            if let Some(proxy_spec) = proxies.get(key) {
                let rel_path = &path[idx+1..];
                let proxy_req = proxy_spec.create_request( &http_client, rel_path, &query, req.headers());

                let reqwest_response = match proxy_req.send().await {
                    Ok(res) => res,
                    Err(err) => {
                        return (StatusCode::BAD_REQUEST, Body::empty()).into_response();
                    }
                };

                Response::builder()
                    .status(reqwest_response.status().as_u16())
                    .body(Body::from_stream(reqwest_response.bytes_stream()))
                    .unwrap()

            } else {
                (StatusCode::BAD_REQUEST, "not proxied").into_response()
            }
        } else {
            (StatusCode::BAD_REQUEST, "not proxied").into_response()
        }
    }


    async fn asset_handler (uri_elems: AxumPath<(String,String)>, req: Request,
                            assets: HashMap<&'static str,LoadAssetFp>) -> Response {
        let AxumPath((key,path)) = uri_elems;

        if let Some(lookup_fn) = assets.get( key.as_str()) {
            let filename = path.as_str();
            match lookup_fn( filename) {
                Ok(bytes) => {
                    get_asset_response( filename, bytes)
                }
                Err(e) => {
                    // TODO - this has to discriminate between not found and extraction error
                    (StatusCode::NOT_FOUND, filename.to_string()).into_response()
                }
            }
        } else { // unknown asset crate
            (StatusCode::NOT_FOUND, "unknown asset category").into_response()
        }
    }

    /// called when receiving AddConnection message
    /// note that we shouldn't block in an await for sending to ourselves
    async fn add_connection(&mut self, hself: ActorHandle<SpaServerMsg>, remote_addr: SocketAddr, ws: WebSocket)->OdinServerResult<()> {
        let raddr = remote_addr.clone();
        let name = raddr.to_string();
        let (mut ws_sender, mut ws_receiver) = ws.split();

        let ws_receiver_task = {
            let hself = hself.clone();
            let remote_addr = remote_addr.clone();

            spawn( &name, async move {
                while let Some(Ok(msg)) = ws_receiver.next().await {
                    match msg.into_text() {
                        Ok(msg) => {
                            if !msg.is_empty() {
                                hself.send_msg( DispatchIncomingWsMsg{remote_addr,ws_msg: msg}).await;
                            }
                        }
                        Err(e) => println!("ignoring binary message")
                    }

                }
                hself.send_msg( RemoveConnection{remote_addr}).await;
            })?
        };

        let conn = SpaConnection { remote_addr, ws_sender, ws_receiver_task };
        self.connections.insert( raddr, conn);
        let conn_ref = self.connections.get_mut( &raddr).unwrap();

        for svc in self.services.iter_mut() { // tell services to send their initial data
            svc.service.init_connection( &hself, svc.is_data_available, conn_ref).await.map_err(|e| connect_error(e))?;
        }

        Ok(())
    }

    fn remove_connection (&mut self, remote_addr: SocketAddr)->OdinServerResult<()> {
        self.connections.remove(&remote_addr);
        Ok(())
    }

    // FIXME - these should use timeouts (we can't have a connection block the server)

    async fn data_available (&mut self, hself: ActorHandle<SpaServerMsg>, sender_id: &'static str, data_type: &'static str)->OdinServerResult<()> {
        let has_connections = self.has_connections();

        for svc in self.services.iter_mut() {
            match svc.data_available( &hself, has_connections, sender_id, data_type).await {
                Ok(true) => svc.is_data_available = true,
                Ok(false) => {}
                Err(e) => error!("data available check failed: {e}")
            }
        }
        Ok(())
    }

    /// called when receiving a DispatchIncomingWsMsg actor message
    async fn dispatch_incoming_ws_msg (&mut self, hself: ActorHandle<SpaServerMsg>, remote_addr: SocketAddr, msg: String)->OdinServerResult<()> {
        if let Some( ws_msg_parts ) = ws_service::extract_ws_msg_parts(&msg) {
            // this is ugly - we have to sequentialize the service loop and the response processing so that we don't keep the mutable self borrow open, 
            // which would prohibit to call broadcast_/send_ws_msg(&mut self,...). The nested loops are just a way to avoid heap allocating the results
            let mut i = 0;
            let n = self.services.len();

            while i < n {
                let mut response: WsMsgReaction = WsMsgReaction::None;

                for svc in &mut self.services[i..] {
                    response = svc.handle_ws_msg( &hself, &remote_addr, &ws_msg_parts).await?;
                    i += 1;
                    if response != WsMsgReaction::None { break }
                }

                match response {
                    WsMsgReaction::Broadcast(m) => self.broadcast_ws_msg(m).await?,
                    WsMsgReaction::Send(m) => self.send_ws_msg( remote_addr, m).await?,
                    WsMsgReaction::None => {}
                }
            }
        }
        Ok(())
    }

    /// send a ws message to all connections.
    /// this does not bail on message delivery failure
    async fn broadcast_ws_msg (&mut self, m: String)->OdinServerResult<()> {
        // TODO - use feed() or send_all() for batches
        let ws_msg = Message::Text(m);
        for conn in self.connections.values_mut() {
            if let Err(e) = conn.ws_sender.send(ws_msg.clone()).await {
                error!("failed to broadcast ws message to {:?}: {}", conn.remote_addr, e);
            }
        }
        Ok(())
    }

    /// send a ws message to the connection of the provided client address
    async fn send_ws_msg (&mut self, remote_addr: SocketAddr, m: String)->OdinServerResult<()> {
        if let Some(conn) = self.connections.get_mut( &remote_addr) {
            if let Err(e) = conn.send( m).await {
                error!("failed to send ws message to {:?}: {}", conn.remote_addr, e);
            }
        }
        Ok(())
    }

    /// called when receiving _Terminate_ message
    fn stop_server (&mut self)->OdinServerResult<()> {
        if let Some(jh) = &self.server_task {
            jh.abort();
            self.server_task = None;
            Ok(())
        } else {
            Err(op_failed("server task not running"))
        }
    }
}


/* #region actor *********************************************************************************************/

#[derive(Debug)]
pub struct AddConnection {
    pub remote_addr: SocketAddr,
    pub ws: WebSocket
}

#[derive(Debug)]
pub struct RemoveConnection {
    pub remote_addr: SocketAddr,
}

#[derive(Debug)]
pub struct DataAvailable {
    pub sender_id: &'static str,
    pub data_type: &'static str,
}

#[derive(Debug)]
pub struct DispatchIncomingWsMsg {
    pub remote_addr: SocketAddr,
    pub ws_msg: String
}

#[derive(Debug)]
pub struct BroadcastWsMsg {
    pub data: String
}

#[derive(Debug)]
pub struct SendWsMsg {
    pub remote_addr: SocketAddr,
    pub data: String
}

define_actor_msg_set! { pub SpaServerMsg = AddConnection | DataAvailable | DispatchIncomingWsMsg | BroadcastWsMsg | SendWsMsg | RemoveConnection }

impl_actor! { match actor_msg for Actor<SpaServer,SpaServerMsg> as
    _Start_ => cont! {
        let hself = self.hself.clone();
        if let Err(e) = self.start_server( hself) {
            error!("failed to start server: {e:?}");
        }
    }
    AddConnection => cont! {
        let hself = self.hself.clone();
        if let Err(e) = self.add_connection( hself, actor_msg.remote_addr, actor_msg.ws).await {
            error!("failed to add connection to {:?}: {:?}", actor_msg.remote_addr, e);
        }
    }
    DataAvailable => cont! {
        let hself = self.hself.clone();
        if let Err(e) = self.data_available( hself, actor_msg.sender_id, actor_msg.data_type).await {
            error!("failed to notify data availability: {e:?}");
        }
    }
    DispatchIncomingWsMsg => cont! {
        let hself = self.hself.clone();
        if let Err(e) = self.dispatch_incoming_ws_msg( hself, actor_msg.remote_addr, actor_msg.ws_msg).await {
            error!("failed to dispatch incoming ws message: {e:?}");
        }
    }
    BroadcastWsMsg => cont! {
        if let Err(e) = self.broadcast_ws_msg( actor_msg.data).await {
            error!("failed to broadcast ws message: {e:?}");
        }
    }
    SendWsMsg => cont! {
        if let Err(e) = self.send_ws_msg( actor_msg.remote_addr, actor_msg.data).await {
            error!("failed to send ws message: {e:?}");
        }
    }
    RemoveConnection => cont! {
        if let Err(e) = self.remove_connection( actor_msg.remote_addr) {
            error!("failed to remove connection to {:?}: {:?}", actor_msg.remote_addr, e);
        }
    }
    _Terminate_ => stop! {
        self.stop_server();
    }
}

/* #endregion actor */

/* #region single page app components ************************************************************************/

#[derive(Debug,PartialEq,Eq)]
pub enum HeaderItem {
    Css(String),
    Script(String),
    Module(String)
}

impl HeaderItem {
    fn append_html (&self, buf: &mut String) {
        match self {
            Self::Css(uri) => write!( buf, "<link rel=\"stylesheet\" type=\"text/css\" href=\"{uri}\"/>\n"),
            Self::Script(uri) => write!( buf, "<script src=\"{uri}\"></script>\n"),
            Self::Module(uri) => write!( buf, "<script type=\"module\" src=\"{uri}\"></script>\n")
        };
    }
}


/// accumulator for components of a single page application, including the parts that make up the document and the routes
/// to serve it (including referenced assets and proxied urls). This data structure is our internal model of
/// the SPA data
define_struct! { pub SpaComponents =

    //--- static document components
    header_items: Vec<HeaderItem> = Vec::new(),
    body_frags: Vec<String>  = Vec::new(),  // HTML elements to add to the body

    //--- components that are used to create the Router

    // service specific routes
    routes: Vec<Box<dyn FnOnce(Router,SpaServerState)->Router + 'static>> = Vec::new(),

    // the URIs we proxy. The key is the symbolic name for the proxied server, the value is the remote URI prefix to use
    proxies: HashMap<String,ProxySpec> = HashMap::new(), // symbolic-name -> ProxySpec

    // asset data to serve - the key is the crate name and the value is a crate-specific function to
    // get the asset data for a filename. Both crate and filename are extracted from the request URI.
    // Note the filename is symbolic - it is what the respective `load_asset(..)` function of the crate
    // uses for lookup
    assets: HashMap<&'static str, fn(&str)->std::result::Result<Bytes,OdinBuildError>> = HashMap::new()
}

/// struct to define how we create requests for proxied URIs
#[derive(Debug,Clone)]
struct ProxySpec {
    uri: String,                     // the target URI to get the data from
    copy_hdrs: Vec<String>,          // header keys to copy from the incoming request
    add_hdrs: Vec<(String,String)>,  // header key/value strings to add
    copy_query: bool,                // shall we copy the query string from the incoming request
    add_query: Option<String>        // query string to add
}

impl ProxySpec {

    fn create_request (&self, http_client: &ClientWithMiddleware, rel_path: &str, query: &RawQuery, hdr_map: &HeaderMap) -> MwRequestBuilder {
        let uri = self.get_uri( rel_path, query);
        let request_builder = http_client.get(uri);

        self.add_headers( request_builder, hdr_map)
    }

    fn get_uri (&self, rel_path: &str, query: &RawQuery) -> String {
        let qs = if let Some(qs) = &query.0 { qs.as_str() } else { "" };
        let add_qs = if let Some(add_qs) = &self.add_query { add_qs.as_str() } else { "" };

        let mut len = self.uri.len() + rel_path.len() + 1 + qs.len() + 1 + add_qs.len() + 1; // just the upper bound
        let mut uri = String::with_capacity(len);
        uri.push_str( &self.uri);

        if rel_path.len() > 0 {
            if !(rel_path.starts_with('?') || rel_path.starts_with('/')) {
                uri.push('/');
            }
            uri.push_str( rel_path);
        }

        if self.copy_query {
            if qs.len() > 0 {
                uri.push('?');
                uri.push_str(qs)
            }
        }

        if add_qs.len() > 0 {
            if qs.len() == 0 { uri.push('?') } else { uri.push('&') }
            uri.push_str( add_qs)
        }

        uri
    }

    fn add_headers (&self, req_builder: MwRequestBuilder, hdr_map: &HeaderMap) -> MwRequestBuilder {
        let mut req_builder = req_builder;

        if !self.copy_hdrs.is_empty() {
            for (k,v) in hdr_map.iter() {
                let key = k.as_str();
                for s in &self.copy_hdrs {
                    if s.eq_ignore_ascii_case(key) {
                        req_builder = req_builder.header(k, v);
                        break;
                    }
                }
            }
        }

        for (k,v) in &self.add_hdrs {
            req_builder = req_builder.header(k, v);
        }

        req_builder
    }
}

impl SpaComponents {

    fn from_svcs (services: &Vec<SpaSvc>)->OdinServerResult<SpaComponents> {
        let mut comps = SpaComponents::new();
        comps.add_intrinsics();

        for svc in services {
            svc.add_components( &mut comps).map_err(|e| init_error(e))?;
        }
        Ok(comps)
    }

    pub fn from (svc_list: &SpaServiceList)->OdinServerResult<SpaComponents> {
        Self::from_svcs(  &svc_list.services)
    }

    fn add_intrinsics (&mut self) {
        self.add_assets( self_crate!(), load_asset); // we always serve odin_server assets
        self.add_module( asset_uri!("main.js")); // we always load main.js
    }

    //--- the functions used to add SpaService components (normally by the `SpaService::add_components()` impl)

    pub fn add_header_item (&mut self, hitem: HeaderItem) {
        if !self.header_items.contains(&hitem) {
            self.header_items.push( hitem);
        }
    }
    pub fn add_css(&mut self, uri: impl ToString) { self.add_header_item( HeaderItem::Css(uri.to_string())) }
    pub fn add_script(&mut self, uri: impl ToString) { self.add_header_item( HeaderItem::Script(uri.to_string())) }
    pub fn add_module(&mut self, uri: impl ToString) { self.add_header_item( HeaderItem::Module(uri.to_string())) }

    pub fn add_body_fragment (&mut self, html: impl ToString) {
        self.body_frags.push( html.to_string())
    }

    pub fn add_route (&mut self, rf: impl FnOnce(Router,SpaServerState)->Router + 'static) {
        self.routes.push( Box::new(rf));
    }

    pub fn add_assets (&mut self, key: &'static str, load_asset_fn: LoadAssetFp) {
        self.assets.insert( key, load_asset_fn);
    }

    pub fn add_proxy (&mut self,
        key: impl ToString,
        uri_base: impl ToString,
        copy_hdrs: Vec<String>,
        add_hdrs: Vec<(String,String)>,
        copy_query: bool,
        add_query: Vec<(String,String)>
    ) {
        let mut uri = uri_base.to_string();

        if uri.ends_with('/') { // canonicalize so that we don't have to check on every use
            uri.pop();
        }

        // turn tuple vector into properly formatted query string once so that we don't have to do this for every request
        let add_query: Option<String> = if add_query.is_empty() {
            None
        } else {
            Some( mk_query_string(add_query.iter()) )
        };

        let proxy_spec = ProxySpec{uri,copy_hdrs,add_hdrs,copy_query,add_query};
        self.proxies.insert( key.to_string(), proxy_spec);
    }

    /// render HTML document. We could use a lib such as build_html but our documents are rather simple so there is no
    /// need for another intermediate doc model
    /// TODO - remove newlines in production
    pub fn to_html(&self, name: &str)->String {
        let mut buf = String::with_capacity(4096);

        write!( buf, "<!DOCTYPE html>\n");
        write!( buf, "<html>\n");
        write!( buf, "<head>\n");

        write!( buf, "<title>{name}</title>\n");
        write!( buf, "<base href=\"{}/\">\n", name);

        for item in &self.header_items {
            item.append_html(&mut buf);
        }

        write!( buf, "</head>\n");
        write!( buf, "<body>\n");

        for frag in &self.body_frags {
            write!( buf, "{frag}\n");
        }

        self.post_init_js_modules(&mut buf);

        write!( buf, "</body>\n");
        write!( buf, "</html>\n");

        buf
    }

    fn post_init_js_modules (&self, buf: &mut String) {
        let module_uris: Vec<&str> = self.header_items.iter()
            .filter_map( |e| if let HeaderItem::Module(uri) = e {Some(uri.as_str())} else {None})
            .collect();

        if !module_uris.is_empty() {
            let mut mod_names: Vec<&str> = Vec::with_capacity(module_uris.len());

            write!( buf, "<script type=\"module\">\n");

            for uri in module_uris.iter() {
                let mod_name = get_file_basename(uri).unwrap();
                mod_names.push(mod_name);
                write!( buf, "import * as {mod_name} from '{uri}';\n");
            }

            for mod_name in mod_names.iter() {
                write!( buf, "if ({mod_name}.postInitialize) {{ {mod_name}.postInitialize(); }}\n");
            }

            write!( buf, "console.log('all js modules initialized');\n");
            write!( buf, "</script>\n");
        }
    }
}

/* #endregion single page app components */
