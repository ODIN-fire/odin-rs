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
#![feature(duration_constructors)]

use odin_build::{self, pkg_cache_dir};
use std::sync::Arc;
use tokio::{self,time::sleep};
use odin_common::define_cli;
use odin_orbital::{load_config, tle_store::{SpaceTrackConfig,SpaceTrackTleStore, TleStore}, OrbitalSatelliteInfo};
use anyhow::{Result};

define_cli! { ARGS [about="TLE retrieval tool"] =
    sat_info: String [help="filename of satellite config"]
}

#[tokio::main]
async fn main() -> Result<()> {
    odin_build::set_bin_context!();

    let config: SpaceTrackConfig = load_config("spacetrack.ron")?;
    let sat_info: Arc<OrbitalSatelliteInfo> = Arc::new( load_config(&ARGS.sat_info)?);
    let cache_dir = pkg_cache_dir!();

    let mut tle_store = SpaceTrackTleStore::new( config, sat_info.clone(), Some(cache_dir));
    print!("pre-fetching TLEs for satellite {}..", sat_info.sat_id);
    let n = tle_store.pre_fetch().await?;
    println!("downloaded {n} TLEs");

    Ok(())
}