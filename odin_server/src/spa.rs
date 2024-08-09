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

use std::{boxed, collections::HashMap, sync::Arc, net::SocketAddr, future::{Future,ready}, path::{PathBuf}, any::type_name, fmt::Write};
use axum::{
    http::{Uri,StatusCode},
    body::Body,
    routing::get, Router,
    extract::{Path, Query, Request, State, connect_info::ConnectInfo},
    middleware::map_request,
    response::{Response,IntoResponse,Html},
    extract::{ws::{Message, WebSocket, WebSocketUpgrade},FromRef, Path as AxumPath}
};
use bytes::Bytes;
use futures_util::{sink::SinkExt, stream::{StreamExt, SplitSink, SplitStream}};
use http_body::Body as _;
use http_body_util::{Full, BodyExt, combinators::UnsyncBoxBody};
use tower::{ServiceExt,BoxError};
use tower_http::services::ServeDir;
use reqwest::{header, Client};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use http_cache_reqwest::{Cache, CacheMode, CACacheManager, HttpCache, HttpCacheOptions};
use serde::{Deserialize,Serialize};
use async_trait::async_trait;

use odin_common::fs::get_file_basename;
use odin_macro::define_struct;
use odin_actor::prelude::*;

use crate::get_asset_response;
use crate::errors::{Result,op_failed};

/// the trait that abstracts a single page application service, which normally represents a visualization
/// layer with its own data (either dynamic or static) and document assets (such as Javascript modules
/// and images) or fragments (HTML elements)
#[async_trait]
pub trait SpaService: Send + Sync + 'static {
    /// override this if the service depends on other services. Default is it doesn't
    fn add_dependencies (&self, sb: SpaServiceListBuilder)->SpaServiceListBuilder {sb} // defaut is no dependencies
    
    /// this adds document fragments and route data for this micro service
    /// Called during server construction to accumulate components of all included SpaServices
    fn add_components (&self, spa: &mut SpaComponents)->Result<()>;

    /// called from server actor after receiving an AddConnection message from the ws route handler  
    /// If data is not owned by service this triggers a data action
    async fn init_connection (&self, hself: &ActorHandle<SpaServerMsg>, remote_addr: SocketAddr) -> Result<()> {
        Ok(())
    }

    /// called from ws input task of respective connection
    async fn handle_incoming_ws_msg (&mut self, msg: Arc<String>) -> Result<()> {
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
struct SpaConnection {
    remote_addr: SocketAddr,
    ws_sender: SplitSink<WebSocket,Message>, // used to send through the websocket
    ws_receiver_task: JoinHandle<()> // the task that (async) reads from the websocket
}

/// the actor state for a single page application server actor
pub struct SpaServer {
    config: SpaServerConfig,
    name: String, // this is not from the config so that we can have the same for different apps
    services: Vec<Box<dyn SpaService>>,

    connections: HashMap<SocketAddr,SpaConnection>, // updated when receiving an AddConnection actor message
    server_task: Option<JoinHandle<()>> // for the server task itself, initialized upon _Start_
}

#[derive(Deserialize,Serialize,Debug)]
pub struct SpaServerConfig {
    pub sock_addr: SocketAddr,
    // ..and more to follow
}

impl SpaServer {

    fn new (config: SpaServerConfig, name: impl ToString, services: Vec<Box<dyn SpaService>>)->Self {
        SpaServer { 
            config, 
            name: name.to_string(), 
            services,
            connections: HashMap::new(),
            server_task: None,
        }
    }

    /// called when receiving _Start_ message
    async fn start_server (&mut self, hself: ActorHandle<SpaServerMsg>)->Result<()> {
        if self.server_task.is_none() {
            let sock_addr = self.config.sock_addr.clone();
            let router = self.build_router( &hself)?.into_make_service_with_connect_info::<SocketAddr>();

            self.server_task = Some( tokio::spawn( async move {
                let listener = tokio::net::TcpListener::bind(sock_addr).await.unwrap();
                axum::serve( listener, router).await.unwrap();    
            }));
            Ok(())
        } else {
            Err(op_failed("server task already running"))
        }
    }

    fn build_router (&self, hself: &ActorHandle<SpaServerMsg>)->Result<Router> {
        let comps = SpaComponents::from( &self.services)?;
        let document = comps.to_html( self.name.as_str());
        let proxies = comps.proxies;
        let assets = comps.assets;
        
        let router = Router::new()
            //--- the document route
            .route( self.name.as_str(), get({
                move |req: Request| { Self::doc_handler(req,document) }
            }))

            //--- the proxy route
            // 'key' is the symbolic server name
            .route( &format!("{}/proxied/:key/*unmatched", self.name), get({
                let mode = CacheMode::Default;
                let manager = CACacheManager { path: odin_build::cache_dir().join("proxies") };
                let options = HttpCacheOptions::default();    
                let http_client = ClientBuilder::new(Client::new())
                    .with( Cache( HttpCache {mode, manager, options}))
                    .build();
                move |uri_elems: Path<(String,String)>, req: Request| { Self::proxy_handler(uri_elems, req, http_client, proxies) }
            }))

            //--- the assets route
            // 'key' is the owning crate
            .route( &format!("{}/assets/:key/*unmatched", self.name), get({
                move |uri_elems: Path<(String,String)>, req: Request| { Self::asset_handler(uri_elems, req, assets)}
            }));

        Ok(router)
    }

    async fn doc_handler (req: Request, doc: String)->Html<String> {
        // TODO - this could discriminate between different user-agents
        Html(doc)
    }

    async fn proxy_handler (uri_elems: Path<(String,String)>, req: Request, 
                            http_client: ClientWithMiddleware, proxies: HashMap<String,String>) -> Response {
        let AxumPath((key,path)) = uri_elems;
        println!("serving proxy for host-name {key}: {path}");
        if let Some(uri) = proxies.get(&key) {
            let uri = format!("{uri}/{path}");
            println!("  - forwarding to proxy {uri}");
    
            let reqwest_response = match http_client.get( uri).send().await {
                Ok(res) => res,
                Err(err) => {
                    println!("request failed");
                    return (StatusCode::BAD_REQUEST, Body::empty()).into_response();
                }
            };
    
            let response_builder = Response::builder().status(reqwest_response.status().as_u16());
            response_builder
                .body(Body::from_stream(reqwest_response.bytes_stream()))
                .unwrap()
    
        } else {
            (StatusCode::BAD_REQUEST, "not proxied").into_response()
        }
    }

    async fn asset_handler (uri_elems: Path<(String,String)>, req: Request,
                            assets: HashMap<&'static str, fn(&str)->Result<Bytes>>) -> Response {
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
    async fn add_connection(&mut self, hself: ActorHandle<SpaServerMsg>, remote_addr: SocketAddr, ws: WebSocket)->Result<()> {
        let raddr = remote_addr.clone();
        let name = raddr.to_string();
        let (mut ws_sender, mut ws_receiver) = ws.split();

        let ws_receiver_task = spawn( &name, async move {
            while let Some(Ok(msg)) = ws_receiver.next().await {
                // TBD
            }
        })?;

        let conn = SpaConnection { remote_addr, ws_sender, ws_receiver_task };
        self.connections.insert( raddr, conn);

        for svc in &self.services {
            svc.init_connection( &hself, remote_addr).await?
        }

        Ok(())
    }

    // TODO - these should use timeouts (we can't have a connection block the server)

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
            conn.ws_sender.send(Message::Text(m)).await;
        }
    }

    /// called when receiving _Terminate_ message
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

/* #region actor *********************************************************************************************/

#[derive(Debug)]
pub struct AddConnection { remote_addr: SocketAddr, ws: WebSocket }

#[derive(Debug)]
pub struct BroadcastWsMsg { 
    pub data: String 
}

#[derive(Debug)]
pub struct SendWsMsg { 
    pub remote_addr: SocketAddr, 
    pub data: String 
}

define_actor_msg_set! { pub SpaServerMsg = AddConnection | BroadcastWsMsg | SendWsMsg }

impl_actor! { match msg for Actor<SpaServer,SpaServerMsg> as
    _Start_ => cont! { 
        let hself = self.hself.clone();
        self.start_server( hself).await;
    }
    AddConnection => cont! {
        let hself = self.hself.clone();
        self.add_connection( hself, msg.remote_addr, msg.ws).await;
    }
    BroadcastWsMsg => cont! {
        self.broadcast_ws_msg( msg.data).await;
    }
    SendWsMsg => cont! {
        self.send_ws_msg( msg.remote_addr, msg.data).await;
    }
    _Terminate_ => stop! {
        self.stop_server();
    }
}

/* #endregion actor */

/* #region single page app components ************************************************************************/

/// accumulator for components of a single page application, including the parts that make up the document and the routes
/// to serve it (including referenced assets and proxied urls). This data structure is our internal model of
/// the SPA data
define_struct! { pub SpaComponents = 
    service_types: Vec<&'static str> = Vec::new(), // the micro-services that contributed components

    //--- components that are used to create the document
    // external resources (URLs)
    ext_css:        Vec<String>  = Vec::new(),
    ext_scripts:    Vec<String>  = Vec::new(),

    // own resources (names only - unique-ified upon entry)
    css:            Vec<String>  = Vec::new(),  // own css
    js_modules:     Vec<String>  = Vec::new(),  // own js modules (including config modules)

    // fragments that are taken verbatim (allowing mutliple entries). Note each frag has to be valid HTML
    body_frags:     Vec<String>  = Vec::new(),  // HTML elements to add to the body

    //--- components that are used to create the Router
    // the URIs we proxy. The key is the symbolic name for the proxied server, the value is the remote URI prefix to use
    proxies: HashMap<String,String> = HashMap::new(), 

    // asset data to serve - the key is the crate name and the value is a crate-specific function to
    // get the asset data for a filename. Both crate and filename are extracted from the request URI.
    // Note the filename is symbolic - it is what the respective `load_asset(..)` function of the crate
    // uses for lookup
    assets: HashMap<&'static str, fn(&str)->Result<Bytes>> = HashMap::new()
}


impl SpaComponents {

    fn from (services: &Vec<Box<dyn SpaService>>)->Result<SpaComponents> {
        let mut comps = SpaComponents::new();
        for svc in services {
            svc.add_components( &mut comps)?;
        }
        Ok(comps)
    }

    //--- the functions used to add SpaService components (normally by the `SpaService::add_components()` impl)

    pub fn add_css (&mut self, css: impl ToString) {
        add_unique( &mut self.css, css.to_string());
    }

    pub fn add_js_module (&mut self, module_name: impl ToString) {
        add_unique( &mut self.js_modules, module_name.to_string());
    }

    pub fn add_ext_css (&mut self, css: impl ToString) {
        add_unique( &mut self.ext_css, css.to_string());
    }

    pub fn add_ext_script (&mut self, script: impl ToString) {  // should this be proxied?
        add_unique( &mut self.ext_scripts, script.to_string());
    }

    pub fn add_body_fragment (&mut self, html: impl ToString) {
        self.body_frags.push( html.to_string())
    }

    pub fn add_proxy (&mut self, key: impl ToString, url_prefix: impl ToString) {
        self.proxies.insert( key.to_string(), url_prefix.to_string());
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

        for css in &self.ext_css {
            write!( buf, r#"<link rel="stylesheet" type="text/css" href="{css}"/>\n"#);
        }
        for s in &self.ext_scripts {
            write!( buf, r#"<script src="{s}"></script>\n"#);
        }

        for s in &self.js_modules {
            write!( buf, r#"<script type="module" src="{s}"></script>\n"#);
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

    /// add async module post-init code as a generated script in the form
    /// 
    /// ```
    /// import * as mod_name from mod_name.js;
    /// ...
    /// if (mod_name.postExec) mod_name.postExec();
    /// ...
    /// console.log("js modules initialized");
    /// ```
    /// 
    /// note that imports have to occur first so that all modules have been initialized when
    /// we call postExec() for any of them
    fn post_init_js_modules (&self, buf: &mut String) {
        let mod_names: Vec<&str> = self.js_modules.iter().map( |p| get_file_basename(p).unwrap()).collect();

        write!( buf, r#"\n<script type="module">\n"#);
        for i in 0..mod_names.len() {
            write!( buf, "import * as {} from ./{};\n", mod_names[i], self.js_modules[i]);
        }

        for mod_name in &mod_names {
            write!( buf, "if ({mod_name}.postExec) {mod_name}.postExec();\n");
        }

        write!( buf, "console.log('js modules initialized');\n");
        write!( buf, "</script>\n");
    }
}


fn add_unique ( v: &mut Vec<String>, s: String) {
    if !v.contains(&s) { v. push(s) }
}

/* #endregion single page app components */



