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

use odin_build;
use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_goesr::{GoesrHotspotService, actor::spawn_goesr_hotspot_actors};
 
run_actor_system!( actor_system => {

    let pre_server = PreActorHandle::new( &actor_system, "server", 64);

    let sat_configs = vec![ "goes_18.ron", "goes_19.ron" ];
    let sats = spawn_goesr_hotspot_actors( &mut actor_system, pre_server.to_actor_handle(), &sat_configs, "fdcc")?;

    let _hserver = spawn_pre_actor!( actor_system, pre_server, 
        SpaServer::new(
            odin_server::load_config("spa_server.ron")?,
            "goesr",
            SpaServiceList::new()
                .add( build_service!( => GoesrHotspotService::new( sats)) )
        )
    )?;

    Ok(())
});
