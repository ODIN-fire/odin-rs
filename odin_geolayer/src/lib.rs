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

use std::{net::SocketAddr,any::type_name, path::{Path,PathBuf}, fs, io::{BufReader, Read}, sync::Arc};
use flate2::read::GzDecoder;
use async_trait::async_trait;
use axum::{
    http::StatusCode,
    extract::{Path as AxumPath},
    routing::{Router,get},
    response::{Response,IntoResponse}
};

use odin_build::prelude::*;
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_cesium::{CesiumService, ImgLayerService};
use odin_common::{fs::get_filename_extension};

define_load_config!{}
define_load_asset!{}

/* #region GeoLayerService ***********************************************************************************/

/// this is a resource-only SpaService that provides configurable GeoJSON layers (e.g., buildings, powerlines, etc)

pub fn default_data_dir()->PathBuf {
    pkg_data_dir!()
}

pub struct GeoLayerService {
    /// the directory where to look for geolayer data
    src_dir: Arc<PathBuf>
}

impl GeoLayerService {
    pub fn new( src_dir: impl AsRef<Path>)->Self { 
        GeoLayerService { 
            src_dir: Arc::new(src_dir.as_ref().to_path_buf()) 
        } 
    }

    /// `path` is from the request, `dir` is from the GeoLayerService
    async fn geo_handler (path: AxumPath<String>, dir: Arc<PathBuf>) -> Response {
        let pathname = dir.join( path.as_str());
        // add to watch list, check, send to WS if change
        if pathname.is_file() {
            // check if zip , unzip if zip
            if Some("gz") == get_filename_extension(pathname.to_str().unwrap()) {
                let file = fs::File::open(pathname).unwrap();
                let file = BufReader::new(file);
                let mut contents = GzDecoder::new(file);
                let mut bytes = Vec::new();

                contents.read_to_end(&mut bytes).unwrap();
                (StatusCode::OK, bytes).into_response()

            } else {
                (StatusCode::OK, fs::read(pathname).unwrap()).into_response()
            }
           
        } else { // add 
            (StatusCode::NOT_FOUND, "geo data not found").into_response()
        }
    }

}

impl SpaService for GeoLayerService {
    fn add_dependencies(&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder
            .add( build_service!( => ImgLayerService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()> {
        spa.add_assets( self_crate!(), load_asset);

        spa.add_module( asset_uri!("geolayer_config.js"));
        spa.add_module( asset_uri!("geolayer.js"));

        let dir = self.src_dir.clone();
        spa.add_route( |router, spa_server_state| {
            router.route( &format!("/{}/geolayer-data/{{*unmatched}}", spa_server_state.name.as_str()), get({
                move |path| Self::geo_handler(path, dir.clone())
            }))
        });

        Ok(())
    }
}

/* #endregion GeoLayerService */