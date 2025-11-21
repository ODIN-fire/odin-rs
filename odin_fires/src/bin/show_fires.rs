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

use std::path::PathBuf;

use odin_build::prelude::*;
use odin_common::define_cli;
use odin_actor::{load_config, prelude::*};
use odin_server::prelude::*;
use odin_cesium::{CesiumService, ImgLayerService};
use odin_fires::fire_service::FireService;

run_actor_system!( actor_system => {
    
    spawn_actor!( actor_system, "spa_server", SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "fires",
        SpaServiceList::new()
            .add(build_service!( let svc = FireService::new(odin_fires::load_config("fires.ron")?)? => svc))
    ));

    Ok(())
});