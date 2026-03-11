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

/// example application to show WindNinja computed micro grid wind from an external server

use odin_actor::prelude::*;
use odin_server::prelude::*;
use odin_common::vec_boxed;

use odin_wx::{WxServiceList,WxFileAvailable};
use odin_openmeteo::{actor::OpenMeteoActor,OpenMeteoConfig,OpenMeteoService};
use odin_hrrr::{self, HrrrActor, HrrrConfig, HrrrService, schedule::{HrrrSchedules,get_hrrr_schedules}};

use odin_wind::{
    actor::{WindActor, WindActorMsg},
    server_client::WindServerClient,
    ForecastStore, Forecast,
    server::{WindServer,WindServerMsg, wind_server_subscribe_action, wind_server_update_action}
};

run_actor_system!( actor_system => {
    let pre_server = PreActorHandle::new( &actor_system, "server", 64);
    let pre_wx = PreActorHandle::new( &actor_system, "wx", 8);

    let wxs: WxServiceList = vec_boxed![ HrrrService::new_basic( pre_wx.to_actor_handle()) ];

    let hwind = spawn_actor!( actor_system, "wind", WindActor::new(
        odin_wind::load_config("wind.ron")?,
        wxs,
        wind_server_subscribe_action( pre_server.to_actor_handle()),
        wind_server_update_action( pre_server.to_actor_handle())
    ))?;

    let hwx = spawn_pre_actor!( actor_system, pre_wx, HrrrActor::with_statistic_schedules(
        odin_hrrr::load_config( "hrrr_conus-8.ron")?,
        data_action!( let hwind: ActorHandle<WindActorMsg> = hwind.clone() => |data: WxFileAvailable| {
            Ok( hwind.try_send_msg( data)? )
        })
    ).await? )?;

    let hserver = spawn_pre_actor!( actor_system, pre_server, WindServer::new(
        odin_wind::load_config("wind_server.ron")?,
        "wind",
        hwind
    ))?;

    Ok(())
});
