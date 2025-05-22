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

use odin_actor::errors::op_failed;
use odin_build::{self, pkg_cache_dir};
use std::sync::Arc;
use tokio::{self,time::sleep};
use chrono::{DateTime,Utc};
use satkit;
use odin_common::{datetime::{parse_optional_datetime_or,utc_now}, define_cli};
use odin_orbital::{
    init_orbital_data, instant_from_datetime, load_config, 
    OrbitInfo, OrbitalSatelliteInfo, TleStore,
    tle_store::{SpaceTrackConfig,SpaceTrackTleStore}, errors::OdinOrbitalError, 
};
use anyhow::{Result};

define_cli! { ARGS [about="calculate orbit info for given satellite"] =
    date: Option<String> [help="datetime spec", long,short],
    sat_info: String [help="filename of SatelliteInfo config"]
}

#[tokio::main]
async fn main() -> Result<()> {
    odin_build::set_bin_context!();

    let dir = pkg_cache_dir!();
    init_orbital_data()?;

    let config: SpaceTrackConfig = load_config("spacetrack.ron")?;
    let sat_info: Arc<OrbitalSatelliteInfo> = Arc::new( load_config( &ARGS.sat_info)?);
    let mut tle_store = SpaceTrackTleStore::new( config, sat_info.clone(), Some(dir));

    let date = parse_optional_datetime_or( &ARGS.date, || utc_now());

    let tle = tle_store.get_tle_for_instant( instant_from_datetime(date)).await?;
    println!("----- TLE:\n{:#?}", tle);

    let t1 = std::time::Instant::now();
    let oi = OrbitInfo::new( sat_info.sat_id, sat_info.step_dur(), tle);
    let t2 = std::time::Instant::now();
    //println!("@@ dt: {}", (t2 - t1).as_micros());
    println!("----- OrbitInfo:\n{:#?}", oi);

    Ok(())
}