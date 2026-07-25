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

use std::{path::{PathBuf},sync::Arc, collections::HashMap};
use axum::{
    http::{Uri,StatusCode},
    body::Body,
    routing::{Router,get},
    extract::{Path as AxumPath, Query},
    response::{Response,IntoResponse},
};
use async_trait::async_trait;
use odin_build::pkg_data_dir;
use odin_common::fs::EnvPathBuf;
use odin_server::prelude::*;
use odin_cesium::ImgLayerService;
use odin_gdal::geods_srv::{WmsQuery,get_wms_handler};

use crate::{CACHE_DIR, LandfireConfig, errors::{Result,OdinLandfireError}, load_asset};


pub struct LandfireService {
    config: LandfireConfig
}

impl LandfireService {
    pub fn new (config: LandfireConfig)->LandfireService {
        LandfireService { config }
    }

    async fn tms_handler ( AxumPath((tms_root,request_path)): AxumPath<(String,String)>, tms_layers: Arc<HashMap<String,EnvPathBuf>>) -> Response {
        if let Some(dir) = tms_layers.get( tms_root.as_str()) {
            let path = dir.join(request_path.as_str());
            if path.is_file() {
                (StatusCode::OK, std::fs::read(path).unwrap()).into_response()
            } else {
                (StatusCode::NOT_FOUND, "tile not found").into_response()
            }
        } else {
            (StatusCode::NOT_FOUND, "unknown TMS service").into_response()
        }
    }

    async fn data_handler (cmap: AxumPath<String>, cmaps: Arc<HashMap<String,EnvPathBuf>>) -> Response {
        if let Some(path) = cmaps.get(cmap.as_str()) {
            if path.is_file() {
                (StatusCode::OK, std::fs::read(path).unwrap()).into_response()
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, "color map not found").into_response()
            }

        } else {
            (StatusCode::NOT_FOUND, "unknown color map").into_response()
        }
    }
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

        //--- WMS (very inefficient for display purposes)
        if !self.config.wms_layers.is_empty() {
            let cache_dir = Arc::new(CACHE_DIR.clone());
            let layer_paths = Arc::new( self.config.wms_layers.clone());
            let capability_path = self.config.wms_capabilities.as_ref().map( |p| Arc::new( p.clone() ));

            spa.add_route( |router, spa_server_state| {
                router.route( &format!("/{}/landfire/wms", spa_server_state.name.as_str()), get({
                    move |query: Query<WmsQuery>| { get_wms_handler( query, layer_paths.clone(), capability_path.clone(), cache_dir.clone()) }
                }))
            });
        }

        //--- TMS
        if !self.config.tms_layers.is_empty() {
            let tms_layers = Arc::new(self.config.tms_layers.clone());
            spa.add_route( |router, spa_server_state| {
                router.route( &format!("/{}/landfire/tms/{{tms_root}}/{{*unmatched}}", spa_server_state.name.as_str()), get({
                    move |path_elems| Self::tms_handler( path_elems, tms_layers.clone())
                }))
            });
        }

        if !self.config.color_maps.is_empty() {
            let cmaps = Arc::new(self.config.color_maps.clone());
            spa.add_route( |router, spa_server_state| {
                router.route( &format!("/{}/landfire/data/{{*unmatched}}", spa_server_state.name.as_str()), get({
                    move |cmap| Self::data_handler( cmap, cmaps.clone())
                }))
            });
        }

        Ok(())
    }
}
