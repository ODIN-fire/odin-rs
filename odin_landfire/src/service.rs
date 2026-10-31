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

use std::{collections::HashMap, sync::Arc};
use axum::{
    http::{HeaderMap, Uri},
    routing::get,
    extract::{Path as AxumPath, Query},
};
use async_trait::async_trait;
use odin_common::fs::EnvPathBuf;
use odin_server::prelude::*;
use odin_cesium::ImgLayerService;
use odin_gdal::wms_srv::WmsService;

use crate::{CACHE_DIR, LandfireConfig, load_asset, lf_cmap::read_lf_cmap};

/// SpaService for display of Landfire data sets. The WMS functionality is provided by odin_gdal::wms_srv::WmsService,
/// we just provide the configuration, the landfire.gov CSV color map reader and the routes
pub struct LandfireService {
    wms: Arc<WmsService>,
    datasets: HashMap<String,EnvPathBuf>,
}

impl LandfireService {
    pub fn new (config: LandfireConfig)->LandfireService {
        let LandfireConfig { wms: wms_config, datasets } = config;
        let wms = Arc::new( WmsService::new( wms_config, CACHE_DIR.join("wms"), Some(&read_lf_cmap)));

        LandfireService { wms, datasets }
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

        //--- WMS GetMap/GetCapabilities
        let wms = self.wms.clone();
        spa.add_route( |router, spa_server_state| {
            router.route( &format!("/{}/landfire/wms", spa_server_state.name.as_str()), get({
                move |Query(q): Query<HashMap<String,String>>, headers: HeaderMap, uri: Uri| {
                    let wms = wms.clone();
                    async move { wms.handle_wms( &q, &headers, &uri).await }
                }
            }))
        });

        //--- color map legends of categorical layers, generated from the CSV color maps
        let wms = self.wms.clone();
        spa.add_route( |router, spa_server_state| {
            router.route( &format!("/{}/landfire/cmap/{{layer}}", spa_server_state.name.as_str()), get({
                move |AxumPath(layer): AxumPath<String>| {
                    let wms = wms.clone();
                    async move { wms.handle_legend( &layer) }
                }
            }))
        });

        Ok(())
    }
}
