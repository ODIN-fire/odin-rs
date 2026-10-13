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

#![allow(unused)]

use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_cesium::{self,ImgLayerService};
use odin_landfire::{self,service::LandfireService};

run_actor_system!( actor_system => {
    let lf_config = odin_landfire::load_config("landfire.ron")?;

    spawn_actor!( actor_system, "spa_server", SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "show_landfire",
        SpaServiceList::new()
            .add( build_service!( => ImgLayerService::new())) // this automatically includes Cesium and UI services
            .add( build_service!( => LandfireService::new( lf_config)))
    ));

    Ok(())
});
