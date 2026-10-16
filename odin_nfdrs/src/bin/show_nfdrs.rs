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

/// single page web application to compute and display NFDRS grid products for interactively selected areas

use std::str::FromStr;
use odin_actor::prelude::*;
use odin_common::define_cli;
use odin_server::prelude::*;
use odin_share::prelude::*;
use odin_openmeteo::{self, ids::ModelId};
use odin_landfire;
use odin_nfdrs::{NfdrsService, NfdrsActor, load_config, actor::{server_update_action, server_remove_action}};

define_cli! { ARGS [about="compute and show NFDRS grid products"] =
    config: String [help="filename of NFDRS config file", long, default_value="nfdrs.ron"],
    wx_config: String [help="filename of OpenMeteo config file", long, default_value="openmeteo.ron"],
    model: String [help="name of openmeteo wx model", long, default_value="ncep_hrrr_conus"],
    lf_src: String [help="name of landfire source", long, default_value="lf_2025_conus.ron"]
}

run_actor_system!( actor_system => {
    let pre_server = PreActorHandle::new( &actor_system, "server", 64);
    let pre_nfdrs = PreActorHandle::new( &actor_system, "nfdrs", 8);

    // spawn a shared store actor so that we can use shared areas of interest
    let hshare = spawn_server_share_actor(&mut actor_system, "share", pre_server.to_actor_handle(), default_shared_items(), false)?;

    let nfdrs_config = load_config( &ARGS.config)?;
    let wx_config = odin_openmeteo::load_config( &ARGS.wx_config)?;
    let lf_src = odin_landfire::load_config( &ARGS.lf_src)?;
    let wx_model = ModelId::from_str( &ARGS.model)?;

    let hnfdrs = spawn_pre_actor!( actor_system, pre_nfdrs,
        NfdrsActor::new(
            nfdrs_config, wx_config, wx_model, lf_src,
            server_update_action( pre_server.to_actor_handle()),
            server_remove_action( pre_server.to_actor_handle())
        )
    )?;

    let _hserver = spawn_pre_actor!( actor_system, pre_server, SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "nfdrs",
        SpaServiceList::new()
            .add( build_service!( let hnfdrs = hnfdrs.clone() => NfdrsService::new( hnfdrs) ))
            .add( build_service!( let hshare = hshare.clone() => ShareService::new( "odin_share_schema.js", hshare)) )
    ))?;

    Ok(())
});
