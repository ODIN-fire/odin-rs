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
use chrono::{Timelike, Utc};
use tokio;

use odin_common::{define_cli};
use odin_fems::{load_config, FemsConfig, get_stations};

define_cli! { ARGS [about="download FEMS station meta-data"] =
    config_name: String [help="name of config file", default_value="fems.ron"]
}

#[tokio::main]
async fn main ()->Result<()> {
    let client = Client::new();
    let config: FemsConfig = load_config(&ARGS.config_name)?;

    let mut stations = get_stations( &client, &config).await?;
    println!("{} stations downloaded", stations.len());

    for (id,station) in stations.iter() {
        println!("{:#?}", station);
    }

    /*
    if let Some(station) = stations.get_mut( &43404) {
        let start = Utc::now().with_minute(0).unwrap().with_second(0).unwrap();
        match odin_fems::update_station_weather_obs( &client, &config, station, start).await {
            Ok(()) => {
                println!("weather for station {} updated:\n {:?}", station.id, station);
            }
            Err(e) => {
                eprintln!("error updating weather data for station {}: {}", station.id, e);
            }
        }
    }
    */

    Ok(())
}
