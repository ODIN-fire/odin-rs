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

/// example application to show Wind computed micro grid wind

use odin_actor::prelude::*;
use odin_common::define_cli;
use odin_server::prelude::*;
use odin_share::prelude::*;
use odin_hrrr::{self,HrrrActor,HrrrConfig,HrrrFileAvailable,schedule::{HrrrSchedules,get_hrrr_schedules}};
use odin_wind::{self, 
    actor::{WindActor,WindActorMsg, AddClientResponse, server_subscribe_action, server_update_action}, 
    ForecastStore, Forecast, 
    wind_service::WindService
};

run_actor_system!( actor_system => {
    let pre_server = PreActorHandle::new( &actor_system, "server", 64);
    let pre_hrrr = PreActorHandle::new( &actor_system, "hrrr", 8);

    // spawn a shared store actor - the JS module only allows forecast region requests for shared GeoRects
    let hshare = spawn_server_share_actor(&mut actor_system, "share", pre_server.to_actor_handle(), default_shared_items(), false)?;

    let hwind = spawn_actor!( actor_system, "wind", WindActor::new(
        odin_wind::load_config("wind.ron")?,
        pre_hrrr.to_actor_handle(),
        server_subscribe_action( pre_server.to_actor_handle()),
        server_update_action( pre_server.to_actor_handle()) 
    ))?;

    let hrrr = spawn_pre_actor!( actor_system, pre_hrrr, HrrrActor::with_statistic_schedules(
        odin_hrrr::load_config( "hrrr_conus-8.ron")?,
        data_action!( let hwind: ActorHandle<WindActorMsg> = hwind.clone() => |data: HrrrFileAvailable| {
            Ok( hwind.try_send_msg( data)? )
        })
    ).await? )?;

    let hserver = spawn_pre_actor!( actor_system, pre_server, SpaServer::new(
        odin_server::load_config("spa_server.ron")?,
        "wind",
        SpaServiceList::new()
            .add( build_service!( let hshare = hshare.clone() => ShareService::new( "odin_share_schema.js", hshare)) )
            .add( build_service!( => WindService::new( hwind) ))
    ))?;

    Ok(())   
});