/*
 * Copyright © 2026, United States Government, as represented by the Administrator of
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

use std::{path::{Path,PathBuf}};
use anyhow::{anyhow,Result};
use reqwest::Client;
use chrono::Utc;
use tokio;

use odin_common::{define_cli, datetime::{hours,full_hour}};
use odin_fems::{FemsConfig, download_weather_obs, load_config, obs_timeframe, station_weather_obs_path};

define_cli! { ARGS [about="download weather observation for FEMS station"] =
    output_dir: Option<String> [help="directory where to store files", short, long],
    id: u32 [help="station id"]
}

#[tokio::main]
async fn main ()->Result<()> {
    let client = Client::new();
    let config: FemsConfig = load_config("fems.ron")?;

    let ref_time = full_hour(&Utc::now());
    let forecast_hours = config.forecast_hours;
    let (start,end) = obs_timeframe( ref_time, forecast_hours);
    let path = station_weather_obs_path( ARGS.id, ref_time, forecast_hours);

    let len = download_weather_obs( &client, &config.url, &path, ARGS.id, start, end).await?;
    println!("saved weather observation for station {} to {:?} ({} bytes)", ARGS.id, path, len);
    Ok(())
}
