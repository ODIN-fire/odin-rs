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
use chrono::{DateTime,Utc};
use odin_common::{datetime::{parse_optional_datetime_or,utc_now}, define_cli};
use odin_orbital::{instant_from_datetime, load_config, orbitinfo::OrbitInfo, tle_store::{SpaceTrackConfig,SpaceTrackTleStore, TleStore}};
use anyhow::{Result};

define_cli! { ARGS [about="calculate orbit info for given satellite"] =
    date: Option<String> [help="datetime spec", long,short],
    satellite: u32 [help="NORAD_CAT_ID for satellite to analyze"]
}

#[tokio::main]
async fn main() -> Result<()> {
    odin_build::set_bin_context!();

    let config = load_config("spacetrack.ron")?;
    let cache_dir = odin_build::cache_dir().join("orbital");
    let mut tle_store = SpaceTrackTleStore::new( config, Some(cache_dir));

    let sat_id = ARGS.satellite;
    let date = parse_optional_datetime_or( &ARGS.date, || utc_now());

    let tle = tle_store.get_tle_for_instant(sat_id, instant_from_datetime(date)).await?;
    println!("{:#?}", tle);

    let t1 = std::time::Instant::now();
    let oi = OrbitInfo::new(sat_id, tle);
    let t2 = std::time::Instant::now();
    //println!("@@ dt: {}", (t2 - t1).as_micros());
    //println!("{oi:#?}");

    Ok(())
}