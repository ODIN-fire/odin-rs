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
use odin_common::geo::GeoRect;
use odin_actor::prelude::*;
use odin_hrrr::{self,HrrrActor,HrrrConfig,HrrrFileAvailable,schedule::{HrrrSchedules,get_hrrr_schedules}};
use odin_windninja::{actor::{WindNinjaActor,WindNinjaActorMsg,AddWindNinjaClient,RemoveWindNinjaClient}, ForecastStore, Forecast};

run_actor_system!( actor_system => {
    let pre_hrrr = PreActorHandle::new( &actor_system, "hrrr", 8);

    let hwind = spawn_actor!( actor_system, "wind", WindNinjaActor::new(
        odin_windninja::load_config("windninja.ron")?,
        pre_hrrr.to_actor_handle(),
        no_dataref_action(), // no init action as we start empty
        dataref_action!( => |forecast: &Forecast| {
            println!("forecast available: {forecast:?}");
            Ok(())
        })
    ))?;

    let hrrr = spawn_pre_actor!( actor_system, pre_hrrr, HrrrActor::with_statistic_schedules(
        odin_hrrr::load_config( "hrrr_conus.ron")?,
        data_action!( let hwind: ActorHandle<WindNinjaActorMsg> = hwind.clone() => |data: HrrrFileAvailable| {
            Ok( hwind.try_send_msg( data)? )
        })
    ).await? )?;

    // test driver - this will kick off computation
    hwind.try_send_msg( AddWindNinjaClient::new("BigSur",GeoRect::from_wsen_degrees( -122.043, 35.99, -121.231, 36.594)))?;

    Ok(())
});