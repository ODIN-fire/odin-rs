/*
 * Copyright 2026 Peter Mehlitz
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://apache.org
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! support for running a GeoDsServer


use std::{collections::HashMap, default::Default, error::Error, net::{IpAddr, SocketAddr}, path::{Path,PathBuf}, sync::Arc, time::SystemTime};

use axum::{
    body::Body,
    extract::{MatchedPath,Query,Json},
    http::{Request, StatusCode as AxStatusCode},
    response::{Html,IntoResponse},
    Router,
    routing::{get,post}
};
use http::StatusCode;
use serde::{Serialize,Deserialize};
use serde_json;
use tokio::{net::TcpListener, task::AbortHandle};
use tower_http::{
    classify::{ServerErrorsAsFailures, SharedClassifier},
    trace::TraceLayer,
    cors::{Any,CorsLayer}
};
use tracing::{info_span, Level, Span};
use tracing_subscriber::{filter, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use odin_build::set_bin_context;
use odin_common::{define_serde_struct, define_cli, fs::{self,EnvPathBuf}, BoundingBox, strings::{deserialize_arr4,parse_array}, if_let, datetime };
use odin_image::overlay_imgs;
use odin_server::{spawn_server_task,ServerConfig, TlsConfig, server_error};

use crate::{
    CACHE_DIR, create_wh_image_from_ds, errors::{OdinGdalError, Result, invalid_filename, misc_error}, geods::{GeoDsImgType, GeoDsSrs, get_local_res_file_path, get_local_wh_file_path, get_local_wh_overlay_file_path, read_res_file, read_values, read_wh_file}
};


// the server configuration data
define_serde_struct! { pub GeoDsServerConfig: Debug =
    layers: HashMap<String,EnvPathBuf>,
    max_cache: u64       [default = "default_max_cache"],

    // the HTTP server settings - we don't use a separate config file here to keep things simple and centralized
    sock_addr: SocketAddr,
    tls: Option<TlsConfig>,
    app_key_file: Option<EnvPathBuf> // if present requests are checked for valid x-app-key headers
}

// WMS server config
define_serde_struct! { pub WmsServerConfig: Debug =
    layers: HashMap<String,EnvPathBuf>,
    capabilities: EnvPathBuf,
    max_cache: u64      [default = "default_max_cache"],

    sock_addr: SocketAddr,
    tls: Option<TlsConfig>,
    app_key_file: Option<EnvPathBuf> // if present requests are checked for valid x-app-key headers
}

fn default_max_cache() -> u64 { 1024*1024*100 } // 100MB - wouldn't be enough if used for map tiles

//--- the WMS queries

/// the subset of WMS requests we support. This is what Cesium.WebMapServiceImageryProvider needs, plus GetCapabilities
#[derive(Debug)]
pub enum WmsRequest {
    GetCapabilities,
    GetMap(WmsGetMap)
}

/// a parsed and normalized WMS GetMap request
#[derive(Debug)]
pub struct WmsGetMap {
    pub version: String,
    pub layers: Vec<String>,
    pub styles: Vec<String>,
    pub srs: GeoDsSrs,
    pub bbox: BoundingBox<f64>, // normalized to x/y order (lon,lat for geographic CRS), regardless of WMS version
    pub width: u32,
    pub height: u32,
    pub img_type: GeoDsImgType,
    pub transparent: bool,
    pub params: HashMap<String,String> // all request parameters with lowercase keys (for vendor specific parameters)
}

/// WMS ServiceException (we only support the XML exception format)
#[derive(Debug)]
pub struct WmsException {
    pub code: Option<&'static str>, // one of the WMS exception codes such as "LayerNotDefined"
    pub msg: String
}

impl WmsException {
    pub fn new (code: Option<&'static str>, msg: impl ToString)->Self { WmsException{ code, msg: msg.to_string() } }
    pub fn missing (param: &str)->Self { Self::new( Some("MissingParameterValue"), format!("missing or invalid {} parameter", param.to_uppercase())) }
}

impl IntoResponse for WmsException {
    fn into_response(self) -> axum::response::Response {
        let code_attr = if let Some(code) = self.code { format!(" code=\"{code}\"") } else { String::new() };
        let body = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<ServiceExceptionReport version="1.3.0" xmlns="http://www.opengis.net/ogc">
  <ServiceException{code_attr}>{}</ServiceException>
</ServiceExceptionReport>
"#, xml_escape( &self.msg));
        (StatusCode::BAD_REQUEST, [(http::header::CONTENT_TYPE, "text/xml")], body).into_response()
    }
}

pub fn xml_escape (s: &str)->String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// parse WMS request parameters. Parameter names are case insensitive according to the WMS spec. We support both
/// WMS 1.1.1 (SRS) and 1.3.0 (CRS) - note that WMS 1.3.0 uses lat,lon axis order for EPSG:4326 BBOX values, which we
/// normalize into x,y order
pub fn parse_wms_request (query: &HashMap<String,String>)->std::result::Result<WmsRequest,WmsException> {
    let params: HashMap<String,String> = query.iter().map( |(k,v)| (k.to_lowercase(), v.clone())).collect();

    if let Some(service) = params.get("service") && !service.eq_ignore_ascii_case("WMS") {
        return Err( WmsException::new( Some("InvalidParameterValue"), "unsupported SERVICE"))
    }

    match params.get("request").map( |s| s.as_str()) {
        Some(r) if r.eq_ignore_ascii_case("GetCapabilities") => Ok( WmsRequest::GetCapabilities ),
        Some(r) if r.eq_ignore_ascii_case("GetMap") => parse_get_map( params).map( |gm| WmsRequest::GetMap(gm)),
        Some(_) => Err( WmsException::new( Some("OperationNotSupported"), "unsupported REQUEST")),
        None => Err( WmsException::missing("request"))
    }
}

fn parse_get_map (params: HashMap<String,String>)->std::result::Result<WmsGetMap,WmsException> {
    let version = params.get("version").cloned().unwrap_or( "1.3.0".to_string());
    let is_v13 = version.starts_with("1.3");

    let srs_spec = if is_v13 { params.get("crs") } else { params.get("srs") }
        .or_else( || params.get("crs")).or_else( || params.get("srs")) // be lenient
        .ok_or( WmsException::missing( if is_v13 {"crs"} else {"srs"}))?;
    let srs = GeoDsSrs::from_srs_spec( srs_spec).ok_or( WmsException::new( Some("InvalidCRS"), format!("unsupported CRS {srs_spec}")))?;

    let bbox = params.get("bbox").and_then( |s| parse_array::<f64,4>( s, ',').ok()).ok_or( WmsException::missing("bbox"))?;
    let bbox = if is_v13 && matches!( srs, GeoDsSrs::WGS84) { // EPSG:4326 in WMS 1.3.0 is lat,lon
        BoundingBox::from_wsen( &[bbox[1], bbox[0], bbox[3], bbox[2]])
    } else {
        BoundingBox::from_wsen( &bbox)
    };
    if bbox.west >= bbox.east || bbox.south >= bbox.north {
        return Err( WmsException::new( Some("InvalidParameterValue"), "invalid BBOX"))
    }

    let width = params.get("width").and_then( |s| s.parse::<u32>().ok()).filter( |w| *w > 0).ok_or( WmsException::missing("width"))?;
    let height = params.get("height").and_then( |s| s.parse::<u32>().ok()).filter( |h| *h > 0).ok_or( WmsException::missing("height"))?;

    let format = params.get("format").ok_or( WmsException::missing("format"))?;
    let img_type = GeoDsImgType::for_mime_type( format).ok_or( WmsException::new( Some("InvalidFormat"), format!("unsupported FORMAT {format}")))?;

    let layers: Vec<String> = params.get("layers").map( |s| s.split(',').filter( |l| !l.is_empty()).map( |l| l.to_string()).collect()).unwrap_or_default();
    if layers.is_empty() { return Err( WmsException::missing("layers")) }
    let styles: Vec<String> = params.get("styles").map( |s| s.split(',').map( |l| l.to_string()).collect()).unwrap_or_default();

    let transparent = params.get("transparent").map( |s| s.eq_ignore_ascii_case("true")).unwrap_or(false);

    Ok( WmsGetMap{ version, layers, styles, srs, bbox, width, height, img_type, transparent, params } )
}

// we also support a non-WMS single layer query API with a choice between
// - fixed width/height grid (-> computed resolution)
// - fixed resolution grid (-> computed width/height)
// - discrete values

/// non-WMS grid query for given width/height
define_serde_struct! { GetWhQuery: Debug =
    layer: String,
    crs: String              [default = "default_crs"],
    bbox:[f64;4]             [deserialize_with="odin_common::strings::deserialize_arr4"],
    width: u32,
    height: u32,
    nodata: Option<f64>,
    format: String           [default = "default_format"]
}

/// non-WMS grid query for given res_x/res_y
define_serde_struct! { GetResQuery: Debug =
    layer: String,
    crs: String              [default = "default_crs"],
    bbox:[f64;4]             [deserialize_with="odin_common::strings::deserialize_arr4"],
    res_x: f64,
    res_y: f64,
    nodata: Option<f64>,
    format: String           [default = "default_format"]
}

/// a non-WMS query to get values for explicit coordinates
define_serde_struct! { GetValuesQuery: Debug =
    layer: String,
    epsg: u32,
    nodata: Option<f64>     // the value to return for NoData source pixels
}

fn default_crs()->String { "EPSG:4326".into() }
fn default_format()->String { "image/tif".into() }
fn default_version()->String { "1.3.0".into() }

/// WMS endpoints (multiple layers)
pub async fn run_wms_server (config_path: String) -> Result<()> {
    let config: Arc<WmsServerConfig> = Arc::new( odin_build::load_config_path( &config_path)?);
    let srv_config: ServerConfig = ServerConfig {
        sock_addr: config.sock_addr.clone(),
        tls: config.tls.clone(),
        heartbeat_interval: None
    };

    let cache_dir = Arc::new(CACHE_DIR.join( "wms"));
    let layer_paths = Arc::new( config.layers.clone());
    let capability_path = Arc::new( config.capabilities.clone());
    fs::ensure_writable_dir( cache_dir.as_ref());

    let mut router = Router::new().route( "/wms", get({
        let layer_paths = layer_paths.clone();
        let capability_path = Some(capability_path.clone());
        let cache_dir = cache_dir.clone();

        move |query: Query<HashMap<String,String>>| { get_wms_handler( query, layer_paths, capability_path, cache_dir) }
    }));

    router = add_cors_layer( router);

    if cfg!(feature="trace_server") {
        router = add_trace_layer( router);
    }

    // start cache cleanup task
    if config.max_cache > 0 {
        spawn_cache_cleanup_task( cache_dir.clone(), config.max_cache);
    }

    println!("serving WMS requests on {}", srv_config.url());
    let server_task = spawn_server_task( &srv_config, router);

    server_task.await.map_err( |e| misc_error(format!("failed to start server task: {e}")))
}

/// our own (single source) endpoints
pub async fn run_server (config_path: String) -> Result<()> {
    let config: Arc<GeoDsServerConfig> = Arc::new( odin_build::load_config_path( &config_path)?);
    let srv_config: ServerConfig = ServerConfig {
        sock_addr: config.sock_addr.clone(),
        tls: config.tls.clone(),
        heartbeat_interval: None
    };

    let cache_dir = Arc::new(CACHE_DIR.clone());
    fs::ensure_writable_dir( cache_dir.as_ref());

    let mut router = Router::new()
        .route( "/GetWhGrid", get({
            let cfg = config.clone();
            let cache_dir = cache_dir.clone();
            move |query:Query<GetWhQuery>| { get_wh_handler( query, cfg, cache_dir) }
        }))
        .route( "/GetResGrid", get({
            let cfg = config.clone();
            let cache_dir = cache_dir.clone();
            move |query:Query<GetResQuery>| { get_res_handler( query, cfg, cache_dir) }
        }))
        .route( "/GetValues", post({
            let cfg = config.clone();
            move |query:Query<GetValuesQuery>, ps:Json<Vec<(f64,f64)>>| { get_values_handler( query, cfg, ps) }
        }));

    router = add_cors_layer( router);

    if cfg!(feature="trace_server") {
        router = add_trace_layer( router);
    }

    // start cache cleanup task
    if config.max_cache > 0 {
        spawn_cache_cleanup_task( cache_dir.clone(), config.max_cache);
    }

    println!("serving GeoDs data on {}", srv_config.url());
    let server_task = spawn_server_task( &srv_config, router);

    server_task.await.map_err( |e| misc_error(format!("failed to start server task: {e}")))
}

fn add_cors_layer (router: Router) -> Router {
    let cors = CorsLayer::new()
        //.allow_origin("http://localhost:9009".parse::<HeaderValue>().unwrap())
        .allow_origin(Any);
    router.layer( cors)
}

fn add_trace_layer (router: Router) -> Router {
    println!("trace server");
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())  // use RUST_LOG to set max level
        //.with_max_level(tracing::Level::DEBUG)
        .try_init();

    router.layer(TraceLayer::new_for_http())
}

fn spawn_cache_cleanup_task (cache_dir: Arc<PathBuf>, max_cache: u64)->Result<AbortHandle> {
    Ok(
        tokio::spawn( async move {
            loop {
                tokio::time::sleep( datetime::hours(1)).await;
                odin_common::fs::purge_lru_files_above_limit( cache_dir.as_ref(), false, max_cache);
            }
        }).abort_handle()
    )
}

//--- WMS route handlers

pub async fn get_wms_handler (Query(q): Query<HashMap<String,String>>, layer_paths: Arc<HashMap<String,EnvPathBuf>>, capabilities_path: Option<Arc<EnvPathBuf>>, cache_dir: Arc<PathBuf>) -> axum::response::Response {
    match parse_wms_request( &q) {
        Ok(WmsRequest::GetMap(gm)) => { // this is the hot path
            if gm.layers.len() > 1 { // the complex case - superimposed layers
                let layers: Vec<EnvPathBuf> = gm.layers.iter().flat_map( |layer| layer_paths.get(layer).cloned() ).collect();
                if layers.len() == gm.layers.len() {
                    serve_multiple_layers( layers, gm.bbox, gm.srs, gm.width, gm.height, None, gm.img_type).await
                } else {
                    WmsException::new( Some("LayerNotDefined"), "unsupported layers").into_response()
                }

            } else { // this is the main path
                if let Some(ds_file) = layer_paths.get( &gm.layers[0]) {
                    serve_single_layer( ds_file.clone(), gm.bbox, gm.srs, gm.width, gm.height, None, gm.img_type).await
                } else {
                    WmsException::new( Some("LayerNotDefined"), "unsupported layer").into_response()
                }
            }
        }
        Ok(WmsRequest::GetCapabilities) => {
            if let Some(capabilities_path) = capabilities_path {
                if capabilities_path.is_file() {
                    odin_server::file_response( capabilities_path.as_ref(), false).await.into_response()
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, "capabilities file not found").into_response()
                }
            } else {
                (StatusCode::NOT_IMPLEMENTED, "no capabilities").into_response()
            }
        }
        Err(e) => e.into_response()
    }
}

fn overlay_images (path: impl AsRef<Path>, imgs: &Vec<PathBuf>)->Result<()> {
    let out_path = path.as_ref().to_path_buf();
    let in_paths: Vec<&PathBuf> = imgs.iter().collect();

    odin_image::overlay_imgs( &in_paths, &out_path)?;

    Ok(())
}


// note that GDAL calls are blocking and hence have to be executed in a spawn_blocking() context so that we don't block the
// async runtime when processing concurrent requests (e.g. map tiles)
async fn serve_single_layer (ds_file: EnvPathBuf, bbox: BoundingBox<f64>, srs: GeoDsSrs, width: u32, height: u32, nodata: Option<f64>, img_type: GeoDsImgType) -> axum::response::Response {
    let res = tokio::task::spawn_blocking( move || read_layer_file( &ds_file, &bbox, srs, width, height, nodata, img_type)).await;
    match res {
        Ok(Ok(path)) =>  odin_server::file_response( &path, false).await.into_response(),
        _ => server_error("failed to create GetMap file").into_response()
    }
}

fn read_layer_file (ds_file: &EnvPathBuf, bbox: &BoundingBox<f64>, srs: GeoDsSrs, width: u32, height: u32, nodata: Option<f64>, img_type: GeoDsImgType)->Result<PathBuf> {
    if let Ok(file_path) = get_local_wh_file_path( ds_file, bbox, srs, width, height, img_type) {
        read_wh_file( &bbox, srs, width, height, nodata, img_type, ds_file, &file_path)?;
        Ok( file_path )
    }else {
        Err( OdinGdalError::MiscError("failed to read layer file".into()))
    }
}

async fn serve_multiple_layers ( layers: Vec<EnvPathBuf>, bbox: BoundingBox<f64>, srs: GeoDsSrs, width: u32, height: u32, nodata: Option<f64>, img_type: GeoDsImgType) -> axum::response::Response {
    let res = tokio::task::spawn_blocking( move || {
        let layer_refs: Vec<&EnvPathBuf> = layers.iter().collect();
        let path = get_local_wh_overlay_file_path( &layer_refs, &bbox, srs, width, height, img_type)?;
        if !path.is_file() {
            let mut layer_files: Vec<PathBuf> = Vec::with_capacity(layers.len());
            for ds_file in &layers {
                match read_layer_file( ds_file, &bbox, srs, width, height, nodata, img_type) {
                    Ok(path) => layer_files.push(path),
                    Err(e) => eprintln!("failed to retrieve layer image {e}")
                }
            }
            if layer_files.is_empty() { return Err( misc_error("failed to retrieve layer files".into())) }
            overlay_images( &path, &layer_files)?;
        }
        Ok(path)
    }).await;

    match res {
        Ok(Ok(path)) => odin_server::file_response( &path, false).await.into_response(),
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "failed to combine layers").into_response()
    }
}

//--- ODIN specific route handlers

async fn get_wh_handler (Query(q): Query<GetWhQuery>, config: Arc<GeoDsServerConfig>, cache_dir: Arc<PathBuf>) -> impl IntoResponse {
    let Some(dem_srs) = GeoDsSrs::from_srs_spec( &q.crs) else { return (StatusCode::BAD_REQUEST, "unsupported target SRS").into_response() };
    let Some(dem_img) = GeoDsImgType::for_mime_type( &q.format) else { return (StatusCode::BAD_REQUEST, "unsupported DEM image type").into_response() };
    let Some(ds_file) = config.layers.get( &q.layer ) else { return (StatusCode::BAD_REQUEST, "unsupported layer").into_response() };

    let bbox = BoundingBox::from_wsen( &q.bbox);
    if let Ok(file_path) = get_local_wh_file_path( ds_file, &bbox, dem_srs, q.width, q.height, GeoDsImgType::TIF) {
        match read_wh_file( &bbox, dem_srs, q.width, q.height, q.nodata, dem_img, ds_file, &file_path) {
            Ok(()) =>  odin_server::file_response( &file_path, true).await.into_response(),
            Err(e) => server_error("failed to create grid file").into_response()
        }
    } else {
        server_error("failed to cache grid file").into_response()
    }
}

async fn get_res_handler (Query(q): Query<GetResQuery>, config: Arc<GeoDsServerConfig>, cache_dir: Arc<PathBuf>) -> impl IntoResponse {
    let Some(dem_srs) = GeoDsSrs::from_srs_spec( &q.crs) else { return (StatusCode::BAD_REQUEST, "unsupported target SRS").into_response() };
    let Some(dem_img) = GeoDsImgType::for_mime_type( &q.format) else { return (StatusCode::BAD_REQUEST, "unsupported DEM image type").into_response() };
    let Some(ds_file) = config.layers.get( &q.layer ) else { return (StatusCode::BAD_REQUEST, "unsupported layer").into_response() };

    let bbox = BoundingBox::from_wsen( &q.bbox);
    if let Ok(file_path) = get_local_res_file_path( ds_file, &bbox, dem_srs, q.res_x, q.res_y, dem_img) {
        match read_res_file( &bbox, dem_srs, q.res_x, q.res_y, q.nodata, dem_img, ds_file, &file_path) {
            Ok(()) =>  odin_server::file_response( &file_path, false).await.into_response(),
            Err(e) => server_error("failed to create grid file").into_response()
        }
    } else {
        server_error("failed to cache grid file").into_response()
    }
}

async fn get_values_handler (Query(q): Query<GetValuesQuery>, config: Arc<GeoDsServerConfig>, mut pos2d: Json<Vec<(f64,f64)>> ) -> impl IntoResponse {
    let Some(srs) = GeoDsSrs::from_epsg(q.epsg) else { return (StatusCode::BAD_REQUEST, "unsupported SRS").into_response() };
    let Some(ds_file) = config.layers.get( &q.layer) else { return (StatusCode::BAD_REQUEST, "unsupported layer").into_response() };

    if let Ok(values) = read_values( ds_file, q.nodata, &srs, pos2d.as_mut_slice()) {
        match serde_json::to_string(&values) {
            Ok(s) => (StatusCode::OK, s).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, "failed to serialize values").into_response()
        }
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, "failed to retrieve values").into_response()
    }
}
