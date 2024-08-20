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

use std::net::SocketAddr;
use odin_common::datetime::epoch_millis;
use async_trait::async_trait;

use odin_build::prelude::*;
use odin_actor::prelude::*;
use odin_server::prelude::*;

define_load_config!{}
define_load_asset!{}

/* #region CesiumService *************************************************************************************/

define_ws_struct!{ SetClock =
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

    fn add_dependencies (&self, spa_builder: SpaServiceListBuilder) -> SpaServiceListBuilder {
        spa_builder
            .add( build_service!( UiService::new()))
            .add( build_service!( WsService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_assets( self_crate!(), load_asset);

        //--- add Cesium (we could turn this into assets to ensure it's there)
        spa.add_proxy( "cesium", "https://cesium.com/downloads/cesiumjs/releases/1.120/Build/Cesium");
        spa.add_script( proxy_uri!( "cesium", "Cesium.js"));
        spa.add_css( proxy_uri!( "cesium", "Widgets/widgets.css"));

        spa.add_css( asset_uri!("odin_cesium.css"));

        //--- add JS modules
        spa.add_module( asset_uri!("odin_cesium_config.js"));
        spa.add_module( asset_uri!("odin_cesium.js"));
        
        //--- add body fragments
        spa.add_body_fragment( r#"<div id="cesiumContainer" class="ui_full_window"></div>"#);

        Ok(())
    }

    async fn init_connection (&self, hself: &ActorHandle<SpaServerMsg>, conn: &mut SpaConnection) -> OdinServerResult<()> {
        let msg = to_json( "odin_cesium/odin_cesium.js", SetClock{time: epoch_millis(), time_scale: 1.0})?;
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

impl SpaService for ImgLayerService {
    fn add_dependencies (&self, spa_builder: SpaServiceListBuilder) -> SpaServiceListBuilder {
        spa_builder.add( build_service!( CesiumService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()>  {
        spa.add_assets( self_crate!(), load_asset);

        spa.add_module( asset_uri!("imglayer_config.js"));
        spa.add_module( asset_uri!("imglayer.js"));

        spa.add_proxy("globe-natgeo", "https://services.arcgisonline.com/ArcGIS/rest/services/NatGeo_World_Map/MapServer");

        Ok(())
    }
}

/* #endregion ImgLayerService */