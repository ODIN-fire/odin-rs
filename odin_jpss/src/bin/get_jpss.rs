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

#![allow(unused)]
use odin_common::{angle::{Angle, LatAngle, LonAngle}, fs::ensure_writable_dir, geo::LatLon};
use reqwest;
use tokio;
use anyhow::{Result, Ok};
use odin_jpss::{RawHotspot, get_latest_jpss, get_query_bounds, read_jpss};
use chrono::Utc;
use http;
use std::{fs, path::PathBuf};
use tempfile;
use std::io::Write as IoWrite;
use csv::Reader;
use odin_build;

#[tokio::main]
async fn main() -> Result<()> {
    let conf = load_config( "jpss_noaa20.ron")?;
    let query_bounds = get_query_bounds(&conf.region);
    let url = format!("{}/usfs/api/area/csv/{}/{}/{}/3", &conf.server, &conf.map_key, &conf.source, &query_bounds);
    let data_dir = odin_config::app_metadata().data_dir.join(&conf.source);
    ensure_writable_dir(&data_dir)?;
    let filename = get_latest_jpss(&query_bounds, &data_dir, &url, &conf.source).await?;
    let hs = read_jpss(&filename)?;
    Ok(())
}