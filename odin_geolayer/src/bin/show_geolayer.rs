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

use std::path::PathBuf;

use odin_build::prelude::*;
use odin_common::define_cli;
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_cesium::{CesiumService, ImgLayerService};
use odin_geolayer::{default_data_dir, GeoLayerService};

define_cli! { ARGS [about="show GeoJSON in ODIN"] =
    data_dir: Option<String> [help="directory where to look for data", short, long]
}

run_actor_system!( actor_system => {
    let data_dir: PathBuf = if let Some(dir) = &ARGS.data_dir { dir.into() } else { default_data_dir() };

    spawn_actor!( actor_system, "spa_server", SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "geolayer",
        SpaServiceList::new()
            .add(build_service!( => GeoLayerService::new( &data_dir))) 
    ));

    Ok(())
});