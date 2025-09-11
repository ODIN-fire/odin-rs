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

use std::{any::type_name, collections::HashMap, net::SocketAddr, sync::Arc, path::{Path,PathBuf}, fs};
use axum::{
    http::StatusCode,
    extract::{Path as AxumPath},
    routing::{Router,get},
    response::{Response,IntoResponse}
};
use async_trait::async_trait;
use serde::Deserialize;

use odin_common::{collections::empty_vec, datetime::epoch_millis, fs::replace_env_var_path, strings::to_string_vec};
use odin_build::prelude::*;
use odin_actor::prelude::*;
use odin_server::prelude::*;

define_load_config!{}
define_load_asset!{}

pub const CESIUM_VERSION: &'static str = "1.133";

/* #region CesiumService *************************************************************************************/

define_ws_payload!{ SetClock =
    time: i64,
    time_scale: f32
}

/// this is a resource-only SpaService that provides basic Cesium plus our view UI
pub struct CesiumService {
    // nothing yet
}

impl CesiumService {
    pub fn new()->Self { CesiumService{} }
}

#[async_trait]
impl SpaService for CesiumService {

    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder
            .add( build_service!( => UiService::new()))
            .add( build_service!( => WsService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_assets( self_crate!(), load_asset);

        //--- add Cesium
        #[cfg(feature="cesium_proxy")]
        {
            spa.add_proxy( "cesium", format!("https://cesium.com/downloads/cesiumjs/releases/{CESIUM_VERSION}/Build/Cesium"));
            spa.add_script( proxy_uri!( "cesium", "Cesium.js"));
            spa.add_css( proxy_uri!( "cesium", "Widgets/widgets.css"));
        }
        #[cfg(feature="cesium_asset")]
        {
            // download *.zip from https://cesium.com/downloads/ and put it into ODIN_ROOT/assets/odin_cesium/
            // rename Cesium/Cesium.js into Cesium/Cesium.min.js - it is already minified 
            spa.add_script( asset_uri!("cesium_base_url.js")); // required since we renamed Cesium.js
            spa.add_script( asset_uri!("cesiumjs/Cesium.min.js"));
            spa.add_css( asset_uri!("cesiumjs/Widgets/widgets.css"));
        }
        #[cfg(feature="cesium_external")]
        { 
            spa.add_script( format!("https://cesium.com/downloads/cesiumjs/releases/{CESIUM_VERSION}/Build/Cesium/Cesium.js"));
            spa.add_css( format!("https://cesium.com/downloads/cesiumjs/releases/{CESIUM_VERSION}/Build/Cesium/Widgets/widgets.css"));
        }

        // unfortunately CesiumWorldTerrain cannot be proxied as it uses a protocol with its own authentication headers and OPTIONS queries

        spa.add_css( asset_uri!("odin_cesium.css"));

        //--- add JS modules
        spa.add_module( asset_uri!("odin_cesium_config.js"));
        spa.add_module( asset_uri!("odin_cesium.js"));

        spa.add_module( asset_uri!("editor_config.js"));
        spa.add_module( asset_uri!("editor.js"));
        
        //--- add body fragments
        spa.add_body_fragment( r#"<div id="cesiumContainer" class="ui_full_window"></div>"#);

        Ok(())
    }

    async fn init_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut WsConnection) -> OdinServerResult<()> {
        let clock = SetClock{time: epoch_millis(), time_scale: 1.0};
        let msg = WsMsg::json( CesiumService::mod_path(), "clock", clock)?;
        conn.send(msg).await;
        Ok(())
    }
}

/* #endregion CesiumService */

/* #region ImgLayerService ***********************************************************************************/

#[derive(Debug,Deserialize)]
pub struct ImgLayerConfig {
    // this allows us to run local TMS services from the same server, i.e. without the need to add CORS headers
    // please note this is a potential bottleneck - use only for a small number of users and for development
    // the entries will be available under `http://<hostname>:<port>/tms/<key>``
    pub tms_map: HashMap<String,PathBuf> // key -> local dir
}

impl ImgLayerConfig {
    fn expand_tms_map (&self) -> HashMap<String,PathBuf> {
        let mut tms_map = HashMap::with_capacity( self.tms_map.len());
        for (k,path) in self.tms_map.iter() {
            if let Ok(path) = replace_env_var_path( &path) {
                if path.is_dir() {
                    tms_map.insert(k.clone(), path);
                } else {
                    eprintln!("local TMS directory does not exist {path:?}");
                }
            } else {
                eprintln!("failed to expand local TMS directory path {path:?}");
            }
        }
        tms_map
    }
}

/// this is a resource-only SpaService that provides a configurable imagery layer (globe tiles plus imagery overlays)
pub struct ImgLayerService {
    config: ImgLayerConfig,

    tms_map: Arc<HashMap<String,PathBuf>>
}

impl ImgLayerService {
    pub fn new ()->Self {
        Self::from( load_config("imglayer.ron").unwrap()) // Ok to panic - this is called from a toplevel ctor
    }

    pub fn from (config: ImgLayerConfig)->Self { 
        let tms_map = Arc::new( config.expand_tms_map());
        ImgLayerService{config, tms_map}
    }

    async fn tms_handler ( AxumPath((tms_root,request_path)): AxumPath<(String,String)>, tms_map: Arc<HashMap<String,PathBuf>>) -> Response {
        if let Some(dir) = tms_map.get( tms_root.as_str()) {
            let path = dir.join(request_path.as_str());
            if path.is_file() {
                (StatusCode::OK, fs::read(path).unwrap()).into_response()
            } else {
                (StatusCode::NOT_FOUND, "tile not found").into_response()
            }
        } else {
            (StatusCode::NOT_FOUND, "unknown TMS service").into_response()
        }
    }
}

// headers to copy from the proxied request for OpenStreetMap tiles - see https://operations.osmfoundation.org/policies/tiles/
// note that requests will fail if we copy all headers
const OSM_HDR: &[&str] = &["user-agent","referer","accept","accept-encoding"]; 

impl SpaService for ImgLayerService {
    fn add_dependencies (&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder.add( build_service!( => CesiumService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()> {
        spa.add_assets( self_crate!(), load_asset);

        spa.add_module( asset_uri!("imglayer_config.js"));
        spa.add_module( asset_uri!("imglayer.js"));

        spa.add_proxy("globe-natgeo", "https://services.arcgisonline.com/ArcGIS/rest/services/NatGeo_World_Map/MapServer");
        spa.add_modified_proxy("globe-osm", "https://tile.openstreetmap.org", to_string_vec(OSM_HDR), empty_vec(), true, empty_vec());
        spa.add_modified_proxy("globe-otm", "https://tile.opentopomap.org", to_string_vec(OSM_HDR), empty_vec(), true, empty_vec());

        if !self.tms_map.is_empty() {
            let tms_map = self.tms_map.clone();
            spa.add_route( |router, spa_server_state| {
                router.route( &format!("/{}/tms/{{tms_root}}/{{*unmatched}}", spa_server_state.name.as_str()), get({
                    move |path_elems| Self::tms_handler( path_elems, tms_map.clone()) 
                }))
            });
        }

        Ok(())
    }
}

/* #endregion ImgLayerService */