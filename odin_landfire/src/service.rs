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

use std::{collections::HashMap, path::PathBuf, sync::{Arc, atomic::{AtomicUsize, Ordering}}};
use axum::{
    http::{HeaderMap, StatusCode, Uri, header},
    routing::get,
    extract::{Path as AxumPath, Query},
    response::{Response, IntoResponse},
};
use async_trait::async_trait;
use tokio::sync::Semaphore;
use odin_common::fs::{self, EnvPathBuf};
use odin_server::prelude::*;
use odin_cesium::ImgLayerService;
use odin_gdal::geods::GeoDsImgType;
use odin_gdal::geods_srv::{WmsException, WmsGetMap, WmsRequest, parse_wms_request};

use crate::{CACHE_DIR, LandfireConfig, load_asset, errors::{OdinLandfireError, Result, op_failed}, wms::{WmsConfig, WmsLayer, capabilities_xml, get_cache_path, load_layers, transparent_png}};

/// max width/height of GetMap images
const MAX_IMG_SIZE: u32 = 2048;

/// purge the image cache after this many new cache entries
const PURGE_INTERVAL: usize = 500;

/// the shared state of our WMS route
struct WmsState {
    config: WmsConfig,
    layers: HashMap<String,WmsLayer>,
    cache_dir: PathBuf,
    n_cached: AtomicUsize,

    // limits the number of concurrent GetMap computations. Requests waiting for a permit are just pending futures, i.e.
    // they are dropped without any work if the client cancels the request (which Cesium does a lot while the camera moves).
    // Tasks spawned with spawn_blocking() cannot be cancelled, i.e. without this limit cancelled requests pile up and
    // run out of file handles
    render_permits: Semaphore,
}

pub struct LandfireService {
    wms: Arc<WmsState>,
    datasets: HashMap<String,EnvPathBuf>,
}

impl LandfireService {
    pub fn new (config: LandfireConfig)->LandfireService {
        let LandfireConfig { wms: wms_config, datasets } = config;

        let layers = load_layers( &wms_config);
        let cache_dir = CACHE_DIR.join("wms");
        let render_permits = Semaphore::new( wms_config.max_renders.max(1));
        let wms = Arc::new( WmsState { config: wms_config, layers, cache_dir, n_cached: AtomicUsize::new(0), render_permits });

        LandfireService { wms, datasets }
    }

    async fn wms_handler (Query(q): Query<HashMap<String,String>>, headers: HeaderMap, uri: Uri, state: Arc<WmsState>) -> Response {
        match parse_wms_request( &q) {
            Ok(WmsRequest::GetMap(gm)) => Self::get_map( gm, state).await,
            Ok(WmsRequest::GetCapabilities) => {
                let xml = capabilities_xml( &state.config, &state.layers, &online_resource( &headers, &uri));
                (StatusCode::OK, [(header::CONTENT_TYPE, "text/xml")], xml).into_response()
            }
            Err(e) => e.into_response()
        }
    }

    async fn get_map (gm: WmsGetMap, state: Arc<WmsState>) -> Response {
        if gm.layers.len() != 1 {
            return WmsException::new( Some("LayerNotDefined"), "only single layer requests are supported").into_response()
        }
        if !state.layers.contains_key( &gm.layers[0]) {
            return WmsException::new( Some("LayerNotDefined"), format!("unknown layer {}", gm.layers[0])).into_response()
        }
        if !matches!( gm.img_type, GeoDsImgType::PNG) {
            return WmsException::new( Some("InvalidFormat"), "only image/png is supported").into_response()
        }
        if gm.width > MAX_IMG_SIZE || gm.height > MAX_IMG_SIZE {
            return WmsException::new( Some("InvalidParameterValue"), format!("max image size is {MAX_IMG_SIZE}")).into_response()
        }

        // don't compute images that are much coarser than the data resolution. Computation time is proportional to the
        // source area, i.e. a few of those would block all render slots for a long time (and with it all the requests
        // waiting for them, including other browser requests to this server since browsers limit the number of connections)
        let layer = &state.layers[ &gm.layers[0]]; // already checked
        if layer.style_for( &gm).is_none() {
            return WmsException::new( Some("StyleNotDefined"), format!("unknown style for layer {}", layer.name)).into_response()
        }
        if layer.is_too_coarse( &gm, state.config.max_downsample) {
            return match transparent_png( gm.width, gm.height) {
                Ok(png) => (StatusCode::OK, [(header::CONTENT_TYPE, "image/png")], png).into_response(),
                Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "failed to create image").into_response()
            }
        }

        // wait for a free render slot (queued in FIFO order). This is dropped if the client cancels the request
        let Ok(_permit) = state.render_permits.acquire().await else {
            return (StatusCode::SERVICE_UNAVAILABLE, "WMS not available").into_response()
        };

        // GDAL calls and file IO are blocking, i.e. we must not execute them on async runtime worker threads
        let res = tokio::task::spawn_blocking({
            let state = state.clone();
            move || get_png( &gm, &state)
        }).await;

        match res {
            Ok(Ok(png)) => (StatusCode::OK, [(header::CONTENT_TYPE, "image/png")], png).into_response(),
            Ok(Err(e)) => {
                eprintln!("landfire WMS GetMap failed: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "failed to create image").into_response()
            }
            Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "image task failed").into_response()
        }
    }

    async fn cmap_handler (AxumPath(layer_name): AxumPath<String>, state: Arc<WmsState>) -> Response {
        let layer_name = layer_name.strip_suffix(".json").unwrap_or( &layer_name);
        if let Some(layer) = state.layers.get( layer_name) && let Some(json) = layer.json_color_map() {
            (StatusCode::OK, [(header::CONTENT_TYPE, "application/json")], json).into_response()
        } else {
            (StatusCode::NOT_FOUND, "unknown color map").into_response()
        }
    }
}

/// get PNG image for request, either from cache or by rendering it from the layer dataset
fn get_png (gm: &WmsGetMap, state: &WmsState)->Result<Vec<u8>> {
    let layer = state.layers.get( &gm.layers[0]).ok_or( op_failed!("unknown layer"))?; // already checked
    let style = layer.style_for( gm).ok_or( op_failed!("unknown style"))?; // already checked
    let path = get_cache_path( &state.cache_dir, layer, style, gm);

    if path.is_file() {
        fs::set_accessed( &path); // for LRU purging
        Ok( std::fs::read( &path)? )

    } else {
        let png = layer.render_png( style, gm)?;

        if let Some(dir) = path.parent() { std::fs::create_dir_all( dir)?; }
        std::fs::write( &path, &png)?;

        if state.n_cached.fetch_add( 1, Ordering::Relaxed) % PURGE_INTERVAL == PURGE_INTERVAL-1 {
            if let Err(e) = fs::purge_lru_files_above_limit( &state.cache_dir, true, state.config.max_cache) {
                eprintln!("failed to purge landfire WMS cache: {e}");
            }
        }
        Ok(png)
    }
}

/// the absolute URL of our WMS endpoint as seen by the client (for capabilities)
fn online_resource (headers: &HeaderMap, uri: &Uri)->String {
    let scheme = headers.get("x-forwarded-proto").and_then( |v| v.to_str().ok()).unwrap_or("http");
    let host = headers.get( header::HOST).and_then( |v| v.to_str().ok()).unwrap_or("localhost");
    format!("{}://{}{}", scheme, host, uri.path())
}

#[async_trait]
impl SpaService for LandfireService {

    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder.add( build_service!( => ImgLayerService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_assets( self_crate!(), load_asset);

        spa.add_module( asset_uri!( "odin_landfire_config.js"));
        spa.add_module( asset_uri!( "odin_landfire.js" ));

        //--- WMS GetMap/GetCapabilities
        let state = self.wms.clone();
        spa.add_route( |router, spa_server_state| {
            router.route( &format!("/{}/landfire/wms", spa_server_state.name.as_str()), get({
                move |query: Query<HashMap<String,String>>, headers: HeaderMap, uri: Uri| { Self::wms_handler( query, headers, uri, state.clone()) }
            }))
        });

        //--- color maps (legends) of categorical layers, generated from the CSV color maps
        let state = self.wms.clone();
        spa.add_route( |router, spa_server_state| {
            router.route( &format!("/{}/landfire/cmap/{{layer}}", spa_server_state.name.as_str()), get({
                move |layer: AxumPath<String>| { Self::cmap_handler( layer, state.clone()) }
            }))
        });

        Ok(())
    }
}
