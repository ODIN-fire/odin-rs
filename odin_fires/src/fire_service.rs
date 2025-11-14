/*
 * Copyright © 2025, United States Government, as represented by the Administrator of 
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

use std::{fs::{self,File}, path::{Path,PathBuf}, io::{BufReader,Read}, sync::Arc};
use flate2::read::GzDecoder;
use axum::{
    http::StatusCode,
    extract::{Path as AxumPath},
    routing::{Router,get},
    response::{Response,IntoResponse}
};
use async_trait::async_trait;
use regex::Regex;
use odin_server::prelude::*;
use odin_actor::prelude::*;
use odin_cesium::{CesiumService, ImgLayerService};
use odin_common::{define_serde_struct, fs::{filepath_contents_as_string, get_filename_extension, matching_files_in_tree}};

use crate::{load_asset, load_summaries, FiresConfig,FireSummary, errors::Result};

pub struct FireService {
    config: FiresConfig,
    summaries: Vec<(PathBuf,FireSummary)>
}

impl FireService {
    pub fn new( config: FiresConfig)->Result<Self> { 
        let regex = Regex::new( &config.summary_pattern)?;

        // we don't use the FireSummary objects here but still want to make sure contents of the files are valid
        let summaries: Vec<(PathBuf,FireSummary)> = load_summaries( &config.src_dir, regex)?;

        Ok( FireService { config, summaries } ) 
    }

    async fn data_handler (path: AxumPath<String>, dir: Arc<PathBuf>) -> Response {
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
            (StatusCode::NOT_FOUND, "history data not found").into_response()
        }
    }
}

#[async_trait]
impl SpaService for FireService {
    fn add_dependencies(&self, spa_builder: SpaServiceList) -> SpaServiceList {
        spa_builder
            .add( build_service!( => ImgLayerService::new()))
    }

    fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()> {
        spa.add_assets( self_crate!(), load_asset);

        spa.add_module( asset_uri!("odin_fires_config.js"));
        spa.add_module( asset_uri!("odin_fires.js"));

        let dir = Arc::new(Path::new( &self.config.src_dir).to_path_buf());
        spa.add_route( |router, spa_server_state| {
            router.route( &format!("/{}/fire-data/{{*unmatched}}", spa_server_state.name.as_str()), get({
                move |path| Self::data_handler(path, dir.clone())
            }))
        });

        Ok(())
    }

    async fn init_connection (&mut self, hself: &ActorHandle<SpaServerMsg>, is_data_available: bool, conn: &mut WsConnection) -> OdinServerResult<()> {
        let remote_addr = conn.remote_addr;

        for (path,summary) in &self.summaries {
            let ws_msg = WsMsg::json( FireService::mod_path(), "fireSummary", summary)?;
            hself.try_send_msg( SendWsMsg{remote_addr,ws_msg})?;
        }

        Ok(())
    }
}
