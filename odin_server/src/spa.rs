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

use std::{boxed, collections::HashMap, sync::{Arc,Mutex}, 
    net::SocketAddr, future::{Future,ready}, time::SystemTime, 
    path::{PathBuf}, any::type_name, fmt::Write,
    result::Result, error::Error
};
use axum::{
    body::Body, 
    extract::{
        connect_info::ConnectInfo, 
        ws::{Message, WebSocket, WebSocketUpgrade}, 
        FromRef, {Path as AxumPath}, Query, RawQuery, Request, State
    }, 
    http::{StatusCode, Uri}, 
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
use reqwest::{header::{self, SET_COOKIE}, Client};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use http_cache_reqwest::{Cache, CacheMode, CACacheManager, HttpCache, HttpCacheOptions};
use serde::{Deserialize,Serialize};
use async_trait::async_trait;

use odin_build::LoadAssetFp;
use odin_common::{fs::get_file_basename,strings};
use odin_macro::define_struct;
use odin_actor::prelude::*;

use crate::{errors::{connect_error, init_error, OdinServerError}, get_asset_response};
use crate::errors::{OdinServerResult,op_failed};

/// the trait that abstracts a single page application service, which normally represents a visualization
/// layer with its own data (either dynamic or static) and document assets (such as Javascript modules
/// and images) or fragments (HTML elements)
#[async_trait]
pub trait SpaService: Send + Sync + 'static {
    /// override this if the service depends on other services. Default is it doesn't
    fn add_dependencies (&self, sb: SpaServiceListBuilder)->SpaServiceListBuilder {sb} // defaut is no dependencies
    
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
    async fn init_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, conn: &mut SpaConnection) -> OdinServerResult<()> {
        Ok(())
    }

    /// can be used to broadcast newly available data to all connections. This is useful if the ws messages to be sent
    /// would be expensive to create and/or we want to store availability state (hence `&mut self`)
    async fn data_available (&mut self, hself: &ActorHandle<SpaServerMsg>, sender_id: &'static str, data_type: &'static str)-> OdinServerResult<()> {
        Ok(())
    }

    /// called from ws input task of respective connection
    async fn handle_incoming_ws_msg (&mut self, msg: String) -> OdinServerResult<()> {
        Ok(())
    }
}

/// an object to build SpaService lists from services that can recursively depend on other services.
/// Each service type is included just once, in the order of first occurrence
pub struct SpaServiceListBuilder {
    seen: Vec<&'static str>,
    services: Vec<Box<dyn SpaService>>
}

impl SpaServiceListBuilder {
    pub fn new ()->Self { SpaServiceListBuilder{seen: Vec::new(), services: Vec::new()} }

    pub fn add<F,T> (self, svc_ctor: F)->Self where F: FnOnce()->T, T: SpaService + 'static {
        let name = type_name::<T>();
        if !self.seen.contains(&name) {
            let svc = svc_ctor();
            let mut sb = svc.add_dependencies( self);
            sb.seen.push(name);
            sb.services.push( Box::new(svc));
            sb
        } else {
            self
        }
    }

    pub fn build (self)->Vec<Box<dyn SpaService>> { 
        self.services
    }
}

/// struct to keep track of active SinglePageApp connections
pub struct SpaConnection {
    pub remote_addr: SocketAddr,
    pub ws_sender: SplitSink<WebSocket,Message>, // used to send through the websocket
    pub ws_receiver_task: JoinHandle<()> // the task that (async) reads from the websocket
}

impl SpaConnection {
    pub async fn send (&mut self, msg: String) {
        self.ws_sender.send( Message::Text(msg)).await;
    }
}

#[derive(Deserialize,Serialize,Debug)]
pub struct TlsConfig {
    pub cert_path: String, // path to PEM encoded certificate
    pub key_path: String,  // path to PEM encoded key data
}

#[derive(Deserialize,Serialize,Debug)]
pub struct SpaServerConfig {
    pub sock_addr: SocketAddr,
    pub tls: Option<TlsConfig>, // if set use TLS (https)
}

/// this is the state that can be passed into axum handlers
/// note this has to clone efficiently
#[derive(Clone)]
pub struct SpaServerState {
    pub name: Arc<String>,
    pub hself: ActorHandle<SpaServerMsg>
    // TODO - should we add Arc<Mutex<HashMap<SocketAddr,SpaConnection>>>> here so that handlers can directly add/send?
}


/// the actor state for a single page application server actor
pub struct SpaServer {
    config: SpaServerConfig,
    name: String, // this is not from the config so that we can have the same for different apps
    services: Vec<Box<dyn SpaService>>,

    connections: HashMap<SocketAddr,SpaConnection>, // updated when receiving an AddConnection actor message
    server_task: Option<JoinHandle<()>> // for the server task itself, initialized upon _Start_
}

impl SpaServer {

    pub fn new (config: SpaServerConfig, name: impl ToString, services: Vec<Box<dyn SpaService>>)->Self {
        SpaServer { 
            config, 
            name: name.to_string(), 
            services,
            connections: HashMap::new(),
            server_task: None,
        }
    }

    fn requires_websocket (&self)->bool {
        self.services.iter().find( |s| s.is_websocket()).is_some()
    }

    /// called when receiving _Start_ message
    async fn start_server (&mut self, hself: ActorHandle<SpaServerMsg>)->OdinServerResult<()> {
        if self.server_task.is_none() {
            
            if cfg!(feature="trace_server") {
                // note this only succeeds if there is no global subscriber set yet
                tracing_subscriber::fmt()
                    .with_env_filter(EnvFilter::from_default_env())  // use RUST_LOG to set max level
                    //.with_max_level(tracing::Level::DEBUG)
                    .try_init();
            }

            let sock_addr = self.config.sock_addr.clone();
            let router = self.build_router( &hself)?.into_make_service_with_connect_info::<SocketAddr>();

            self.server_task = if let Some(tls) = &self.config.tls {
                println!("serving https://{}/{}", self.config.sock_addr, self.name);
                let cert_path = strings::env_expand( &tls.cert_path);
                let key_path = strings::env_expand( &tls.key_path);
                Some( tokio::spawn( async move {
                    let tls_config = RustlsConfig::from_pem_file(PathBuf::from(cert_path), PathBuf::from(key_path)).await.unwrap();
                    axum_server::bind_rustls( sock_addr, tls_config).serve( router).await.unwrap();
                }))

            } else {
                println!("serving http://{}/{}", sock_addr, self.name);
                Some( tokio::spawn( async move {
                    let listener = tokio::net::TcpListener::bind(sock_addr).await.unwrap();
                    axum::serve( listener, router).await.unwrap();    
                }))
            };
            Ok(())

        } else {
            Err(op_failed("server task already running"))
        }
    }

    fn build_router (&self, hself: &ActorHandle<SpaServerMsg>)->OdinServerResult<Router> {
        let comps = SpaComponents::from( &self.services)?;
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
            let spa_server_state = SpaServerState {
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
                            http_client: ClientWithMiddleware, proxies: HashMap<String,String>) -> Response {
        if let Some(idx) = path.find('/') {
            let key = &path[0..idx];

            if let Some(base_uri) = proxies.get(key) {
                let rel_path = &path[idx+1..];
                let uri = Self::get_proxy_uri( base_uri, rel_path, query);
        
                let reqwest_response = match http_client.get( uri).send().await {
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

    fn get_proxy_uri (base_uri: &str, path: &str, query: RawQuery)->String {
        let qs = if let Some(qs) = &query.0 { qs.as_str() } else { "" };

        let len = base_uri.len() + path.len() + 1 + qs.len() + 1;
        let mut uri = String::with_capacity(len);
        uri.push_str( base_uri);

        if path.len() > 0 {
            if !(path.starts_with('?') || path.starts_with('/')) { 
                uri.push('/'); 
            }
            uri.push_str( path);
        }

        if qs.len() > 0 {
            uri.push('?');
            uri.push_str(qs)
        }

        uri
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
                                println!("@@ received ws: {}", msg)
                                // dispatch to respective SpaService handler here
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
            svc.init_connection( &hself, conn_ref).await.map_err(|e| connect_error(e))?;
        }

        Ok(())
    }

    fn remove_connection (&mut self, remote_addr: SocketAddr) {
        self.connections.remove(&remote_addr);
    }

    // TODO - these should use timeouts (we can't have a connection block the server)

    async fn data_available (&mut self, hself: ActorHandle<SpaServerMsg>, sender_id: &'static str, data_type: &'static str) {
        for svc in self.services.iter_mut() {
            svc.data_available( &hself, sender_id, data_type).await;
        }
    }

    // called when receiving a BroadcastWsMsg message
    async fn broadcast_ws_msg (&mut self, m: String) {
        // TODO - use feed() or send_all() for batches
        let ws_msg = Message::Text(m);
        for conn in self.connections.values_mut() {
            conn.ws_sender.send(ws_msg.clone()).await; 
        }
    }

    /// called when receiving a SendWsMsg message
    async fn send_ws_msg (&mut self, remote_addr: SocketAddr, m: String) {
        if let Some(conn) = self.connections.get_mut( &remote_addr) {
            conn.send( m).await;
        }
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
pub struct BroadcastWsMsg { 
    pub data: String 
}

#[derive(Debug)]
pub struct SendWsMsg { 
    pub remote_addr: SocketAddr, 
    pub data: String 
}

define_actor_msg_set! { pub SpaServerMsg = AddConnection | DataAvailable | BroadcastWsMsg | SendWsMsg | RemoveConnection }

impl_actor! { match msg for Actor<SpaServer,SpaServerMsg> as
    _Start_ => cont! { 
        let hself = self.hself.clone();
        self.start_server( hself).await;
    }
    AddConnection => cont! {
        let hself = self.hself.clone();
        self.add_connection( hself, msg.remote_addr, msg.ws).await;
    }
    DataAvailable => cont! {
        let hself = self.hself.clone();
        self.data_available( hself, msg.sender_id, msg.data_type).await;
    }
    BroadcastWsMsg => cont! {
        self.broadcast_ws_msg( msg.data).await;
    }
    SendWsMsg => cont! {
        self.send_ws_msg( msg.remote_addr, msg.data).await;
    }
    RemoveConnection => cont! {
        self.remove_connection( msg.remote_addr);
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
    proxies: HashMap<String,String> = HashMap::new(), 

    // asset data to serve - the key is the crate name and the value is a crate-specific function to
    // get the asset data for a filename. Both crate and filename are extracted from the request URI.
    // Note the filename is symbolic - it is what the respective `load_asset(..)` function of the crate
    // uses for lookup
    assets: HashMap<&'static str, fn(&str)->std::result::Result<Bytes,OdinBuildError>> = HashMap::new()
}

impl SpaComponents {

    pub fn from (services: &Vec<Box<dyn SpaService>>)->OdinServerResult<SpaComponents> {
        let mut comps = SpaComponents::new();
        for svc in services {
            svc.add_components( &mut comps).map_err(|e| init_error(e))?;
        }
        Ok(comps)
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

    pub fn add_proxy (&mut self, key: impl ToString, uri_base: impl ToString) {
        let mut uri = uri_base.to_string();
        if uri.ends_with('/') { // canonicalize so that we don't have to check on every use
            uri.pop();
        }

        self.proxies.insert( key.to_string(), uri);
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



