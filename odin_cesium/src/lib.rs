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

use std::{net::SocketAddr,any::type_name};
use odin_common::{datetime::epoch_millis, strings::to_string_vec, collections::empty_vec};
use async_trait::async_trait;

use odin_build::prelude::*;
use odin_actor::prelude::*;
use odin_server::prelude::*;

define_load_config!{}
define_load_asset!{}

pub const CESIUM_VERSION: &'static str = "1.128";

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

    async fn init_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut SpaConnection) -> OdinServerResult<()> {
        let clock = SetClock{time: epoch_millis(), time_scale: 1.0};
        let msg = WsMsg::json( CesiumService::mod_path(), "clock", clock)?;
        conn.send(msg).await;
        Ok(())
    }
}

/* #endregion CesiumService */

/* #region ImgLayerService ***********************************************************************************/

/// this is a resource-only SpaService that provides a configurable imagery layer (globe tiles plus imagery overlays)
pub struct ImgLayerService {
    // nothing yet
}

impl ImgLayerService {
    pub fn new()->Self { ImgLayerService{} }
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

        Ok(())
    }
}

/* #endregion ImgLayerService */