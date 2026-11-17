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

//! stand-alone NEXRAD Level II display server (imports configured stations into our cache dir and serves the products as WMS layers)

use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_nexrad::{
    load_config, NexradConfig,
    actor::{NexradActor, server_init_action, server_update_action},
    service::{NexradService, NexradWms}
};

run_actor_system!( actor_system => {
    let config: NexradConfig = load_config("nexrad.ron")?;
    let wms = NexradWms::new( &config); // shared between the actor actions and the service

    let pre_server = PreActorHandle::new( &actor_system, "server", 64);

    let actor_id = "nexrad";
    let hnexrad = spawn_actor!( actor_system, actor_id, NexradActor::new(
        config,
        server_init_action( pre_server.to_actor_handle(), actor_id, wms.clone()),
        server_update_action( pre_server.to_actor_handle(), wms.clone())
    ))?;

    let _hserver = spawn_pre_actor!( actor_system, pre_server, SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "nexrad",
        SpaServiceList::new()
            .add( build_service!( let hnexrad = hnexrad.clone(), let wms = wms.clone() => NexradService::new( hnexrad, wms)))
    ))?;

    Ok(())
});
