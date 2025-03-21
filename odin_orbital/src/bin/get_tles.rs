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

use odin_build;
use tokio::{self,time::sleep};
use odin_common::define_cli;
use odin_orbital::{load_config, tle_store::{SpaceTrackConfig,SpaceTrackTleStore, TleStore}};
use anyhow::{Result};

define_cli! { ARGS [about="TLE retrieval tool"] =
    satellites: Vec<u32> [help="list of NORAD_CAT_IDs for satellites to retrieve"]
}

#[tokio::main]
async fn main() -> Result<()> {
    odin_build::set_bin_context!();

    let config = load_config("spacetrack.ron")?;
    let cache_dir = odin_build::cache_dir().join("orbital");

    let mut tle_store = SpaceTrackTleStore::new( config, Some(cache_dir));

    for sat_id in &ARGS.satellites {
        print!("pre-fetching TLEs for satellite {sat_id}..");
        let n = tle_store.pre_fetch( *sat_id).await?;
        println!("{n}.");
    }

    Ok(())
}