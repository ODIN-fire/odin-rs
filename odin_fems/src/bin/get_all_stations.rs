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
#![allow(unused)]

use std::{path::{Path,PathBuf}};
use anyhow::{anyhow,Result};
use reqwest::Client;
use chrono::{Timelike, Utc};
use tokio;

use odin_common::{define_cli, cartographic::Cartographic, cartesian3::Cartesian3};
use odin_fems::{FemsConfig, FemsStation, FemsStationKdTree, load_config, download_all_stations_metadata, parse_station_metadata};

define_cli! { ARGS [about="download and store all FEMS station meta-data"] =
    config_name: String [help="name of config file", default_value="fems.ron"],
    kd_path: Option<String> [help="optional filename to store kd-tree with station metadata", long]
}

#[tokio::main]
async fn main ()->Result<()> {
    let client = Client::new();
    let config: FemsConfig = load_config(&ARGS.config_name)?;

    let path = download_all_stations_metadata( &client, &config.url).await?;
    let stations = parse_station_metadata( &path)?;

    println!("{} stations metadata downloaded to {:?}", stations.len(), path);

    if let Some(kd_path) = &ARGS.kd_path {
        let kd_tree = FemsStationKdTree::new( &stations);
        kd_tree.save_to( &kd_path)?;
        println!("kd-tree saved to {}", kd_path);
    }

    Ok(())
}
