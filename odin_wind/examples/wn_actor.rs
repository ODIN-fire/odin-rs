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

use tokio::main;
use odin_common::{geo::GeoRect, net::ZERO_ADDR, vec_boxed};
use odin_actor::prelude::*;
use odin_wx::{WxFileAvailable,WxServiceList};
use odin_hrrr::{self,HrrrActor,HrrrService,HrrrConfig,schedule::{HrrrSchedules,get_hrrr_schedules}};
use odin_wind::{AddWindClient,SubscribeResponse,WindRegion,actor::{WindActor, WindActorMsg}, errors::Result, Forecast, ForecastStore};

run_actor_system!( actor_system => {
    let pre_wx = PreActorHandle::new( &actor_system, "hrrr", 8);

    let wxs: WxServiceList = vec_boxed![ HrrrService::new_basic( pre_wx.to_actor_handle()) ];
    let hwind = spawn_actor!( actor_system, "wind", WindActor::new(
        odin_wind::load_config("wind.ron")?,
        wxs,
        data_action!( => |res: SubscribeResponse| {
            println!("add client response: {res:#?}");
            Ok(())
        }),
        dataref_action!( => |forecast: &Forecast| {
            println!("forecast available: {forecast:#?}");
            Ok(())
        })
    ))?;

    let hwx = spawn_pre_actor!( actor_system, pre_wx, HrrrActor::with_statistic_schedules(
        odin_hrrr::load_config( "hrrr_conus-1.ron")?,
        data_action!( let hwind: ActorHandle<WindActorMsg> = hwind.clone() => |data: WxFileAvailable| {
            Ok( hwind.try_send_msg( data)? )
        })
    ).await? )?;

    // test driver - this will kick off computation
    let wn_region = WindRegion::new( "region/ca/BigSur", GeoRect::from_wsen_degrees( -122.043, 35.99, -121.231, 36.594));
    hwind.try_send_msg( AddWindClient {wn_region, remote_addr: ZERO_ADDR})?;

    Ok(())
});
