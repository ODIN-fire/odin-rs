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

#[derive(Deserialize,Debug)]
pub struct WmsQuery {
    service: String,
    request: String,
    version: String,

    crs: Option<String>,
    bbox: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    format: Option<String>,
    styles: Option<String>,
    layers: Option<String>,
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

        move |query: Query<WmsQuery>| { get_wms_handler( query, layer_paths, capability_path, cache_dir) }
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

pub async fn get_wms_handler (Query(q): Query<WmsQuery>, layer_paths: Arc<HashMap<String,EnvPathBuf>>, capabilities_path: Option<Arc<EnvPathBuf>>, cache_dir: Arc<PathBuf>) -> impl IntoResponse {
    if q.request == "GetMap" { // this is the hot path
        if let Some(bbox) = q.bbox
        && let Ok(bbox) = parse_array::<f64,4>(&bbox, ',')
        && let Some(width) = q.width
        && let Some(height) = q.height
        && let Some(crs) = q.crs
        && let Some(fmt) = q.format
        && let Some(layers) = q.layers {
            if let Some(srs) = GeoDsSrs::from_srs_spec( &crs)
            && let Some(img_type) = GeoDsImgType::for_mime_type( &fmt) {
                // WMS bbox is min_x,min_y,max_x,max_y (caller has to make sure)
                let bbox = BoundingBox::from_wsen(&bbox);

                if layers.contains(',') { // the complex case - superimposed layers
                    let mut layers = layers.split(',').flat_map( |layer| layer_paths.get(layer) ).collect::<Vec<&EnvPathBuf>>();
                    if !layers.is_empty() {
                        serve_multiple_layers( layers, &bbox, srs, width, height, None, img_type).await.into_response() // WMS does not serve tiffs
                    } else {
                        (StatusCode::BAD_REQUEST, "unsupported layers").into_response()
                    }

                } else { // this is the main path
                    if let Some(ds_file) = layer_paths.get( &layers) {
                        serve_single_layer( ds_file, &bbox, srs, width, height, None, img_type).await.into_response() // WMS does not serve tiffs
                    } else {
                        (StatusCode::BAD_REQUEST, "unsupported layer").into_response()
                    }
                }
            } else {
                (StatusCode::BAD_REQUEST, "unsupported GetMap format or CRS").into_response()
            }
        } else {
            (StatusCode::BAD_REQUEST, "missing WMS GetMap parameters").into_response()
        }

    } else if q.request == "GetCapabilities" {
        if let Some(capabilities_path) = capabilities_path {
            if capabilities_path.is_file() {
                odin_server::file_response( capabilities_path.as_ref(), false).await.into_response()
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, "capabilities file not found").into_response()
            }
        } else {
            (StatusCode::NOT_IMPLEMENTED, "no capabilities").into_response()
        }
    } else {
        (StatusCode::BAD_REQUEST, "unsupported REQUEST param").into_response()
    }
}

fn overlay_images (path: impl AsRef<Path>, imgs: &Vec<PathBuf>)->Result<()> {
    let out_path = path.as_ref().to_path_buf();
    let in_paths: Vec<&PathBuf> = imgs.iter().collect();

    odin_image::overlay_imgs( &in_paths, &out_path)?;

    Ok(())
}


async fn serve_single_layer (ds_file: &EnvPathBuf, bbox: &BoundingBox<f64>, srs: GeoDsSrs, width: u32, height: u32, nodata: Option<f64>, img_type: GeoDsImgType) -> impl IntoResponse {
    match read_layer_file( ds_file, bbox, srs, width, height, nodata, img_type) {
        Ok(path) =>  odin_server::file_response( &path, true).await.into_response(),
        Err(e) => server_error("failed to create GetMap file").into_response()
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

async fn serve_multiple_layers ( layers: Vec<&EnvPathBuf>, bbox: &BoundingBox<f64>, srs: GeoDsSrs, width: u32, height: u32, nodata: Option<f64>, img_type: GeoDsImgType) -> impl IntoResponse {
    if let Ok(path) = get_local_wh_overlay_file_path( &layers, bbox, srs, width, height, img_type) {
        if path.is_file() {
            odin_server::file_response( &path, true).await.into_response()

        } else {
            let mut layer_files: Vec<PathBuf> = Vec::with_capacity(layers.len());
            for ds_file in layers {
                match read_layer_file( ds_file, &bbox, srs, width, height, nodata, img_type) {
                    Ok(path) => layer_files.push(path),
                    Err(e) => eprintln!("failed to retrieve layer image {e}")
                }
            }

            if !layer_files.is_empty() {
                if overlay_images( &path, &layer_files).is_ok() {
                    odin_server::file_response( &path, true).await.into_response()
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, "failed to combine layers").into_response()
                }
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, "failed to retrieve layer files").into_response()
            }
        }
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, "layer filename error").into_response()
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
