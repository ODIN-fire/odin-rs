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
use odin_build;
use odin_action::data_action;
use odin_wx::{WxDataSetRequest,WxFileAvailable};
use odin_hrrr::{
    load_config, run_downloads, schedule::{get_hrrr_schedules, HrrrSchedules}, HrrrConfig, Result
};

define_cli! { ARGS [about="NOAA HRRR download tool"] =
    hrrr_config: String [help="filename of HRRR config file", long,default_value="hrrr_conus-full.ron"],
    statistic_schedules: bool [help="compute schedules of available forecast files from server dir listing", long],
    periodic: bool [help="option to continuously download new forecasts", long],
    ds_configs: Vec<String> [help="filenames of WxDataSetConfig files"]
}

#[tokio::main]
async fn main ()->Result<()> {
    odin_build::set_bin_context!();

    let conf: HrrrConfig = load_config( &ARGS.hrrr_config)?;
    let schedules: HrrrSchedules = get_hrrr_schedules( &conf, ARGS.statistic_schedules).await?;
    //println!("@@ reg: {:?}", schedules.reg);
    //println!("@@ ext: {:?}", schedules.ext);

    let dsrs: Vec<Arc<WxDataSetRequest>> = ARGS.ds_configs.iter().map( |filename| {
        let ds: WxDataSetRequest = match load_config(filename) {
            Ok(ds) => ds,
            Err(e) => panic!("failed to load data set request config file {}: {}", filename, e)
        };
        Arc::new( ds)
    }).collect();

    let file_avail_action = data_action!( => |data: WxFileAvailable| {
        println!("HRRR forecast file available: {:?} for {:?}", data.path.file_name().unwrap(), data.forecasts);
        Ok(())
    });

    run_downloads(conf, dsrs, schedules, ARGS.periodic, file_avail_action).await
}
