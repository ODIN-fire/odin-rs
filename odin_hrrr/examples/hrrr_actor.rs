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

use std::sync::Arc;
use odin_common::define_cli;
use odin_actor::prelude::*;
use odin_hrrr::{load_config,HrrrActor, AddDataSet, HrrrConfig, schedule::{HrrrSchedules,get_schedules}, HrrrDataSetRequest, HrrrDataSetConfig, HrrrFileAvailable};

define_cli! { ARGS [about="NOAA HRRR download example using HrrrActor"] =
    hrrr_config: String [help="filename of HRRR config file", short,long,default_value="hrrr_conus.ron"],
    statistic_schedules: bool [help="compute schedules of available forecast files from server dir listing", short, long],
    ds_config: String [help="filename of HrrrDataSetConfig file"]
}

run_actor_system!( actor_system => {
    let hrrr_config: HrrrConfig = load_config( &ARGS.hrrr_config)?;
    let schedules: HrrrSchedules = get_schedules( &hrrr_config, ARGS.statistic_schedules).await?;
    let ds: HrrrDataSetConfig = load_config( &ARGS.ds_config)?;
    let req = Arc::new(HrrrDataSetRequest::new(ds));
    
    let himporter = spawn_actor!( actor_system, "hrrr_importer", HrrrActor::new(
        hrrr_config,
        schedules,
        data_action!( => |data: HrrrFileAvailable| {
            println!("file available: {:?}", data.path.file_name().unwrap());
            Ok(())
        })
    ))?;

    actor_system.start_all().await?;

    himporter.send_msg( AddDataSet(req)).await?;

    actor_system.process_requests().await?;

    Ok(())
});