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

use tokio::{self,time::sleep};
use std::{fs::File,path::{Path,PathBuf},sync::Arc};
use anyhow::{anyhow,Result};
use chrono::{DateTime,Utc};
use satkit::{Instant,Duration};
use ron;
use odin_build::pkg_cache_dir;
use odin_common::{angle::Angle90,define_cli,fs::set_file_contents};
use odin_orbital::{
    errors::OdinOrbitalError, 
    init_orbital_data, instant_from_datetime_spec, load_config, 
    overpass::OverpassCalculator, 
    tle_store::SpaceTrackTleStore, 
    OrbitalSatelliteInfo
};

define_cli! { ARGS [about="calculate overpasses for given satellite, history and future durations"] =
    date: Option<String> [help="datetime spec (if not specified use current datetime)", long,short],
    past_days: u64 [help="number of past days to compute", short,long, default_value="1"],
    future_days: u64 [help="number of future days to compute", short,long, default_value="1"],
    region: String [help="filename of region", short, long, default_value="conus.ron"],
    max_tles: usize [help="max number of TLEs to store", short,long, default_value="10"],
    sat_info: String [help="filename of satellite to analyze"]
}

#[tokio::main]
async fn main() -> Result<()> {
    odin_build::set_bin_context!();
    let cache_dir = pkg_cache_dir!();
    init_orbital_data();

    let region = load_config( &ARGS.region)?;
    let sat_info: Arc<OrbitalSatelliteInfo> = Arc::new(load_config( &ARGS.sat_info)?);

    let t: Instant = if let Some(ds) = &ARGS.date { instant_from_datetime_spec(ds)? } else { Instant::now() };
    let t_start = t - Duration::from_days( ARGS.past_days as f64);
    let dur = Duration::from_days((ARGS.past_days + ARGS.future_days) as f64);
    let tle_store = SpaceTrackTleStore::new( load_config("spacetrack.ron")?, sat_info.clone(), Some(cache_dir.clone()));

    let mut overpass_calc = OverpassCalculator::new( sat_info.clone(), region, tle_store);

    if let Err(e) = overpass_calc.initialize().await {
        println!("failed to initialize overpass calculator: {e}");
        return Err( anyhow!(e));
    }

    let overpasses = overpass_calc.get_overpasses( t_start, dur, 100).await?; // get up to 100 overpasses
    for o in &overpasses { 
        println!("{o}");

        let df = format!("{}", o.start.format("%Y-%m-%d_%H_%M"));
        let p = cache_dir.join( format!("{}_{}_{}.ron", sat_info.name, sat_info.instrument, df));
        // println!("                     saved to {}", p.display());

        let mut file = File::create(&p)?;
        let s = ron::ser::to_string_pretty(o, ron::ser::PrettyConfig::default().compact_structs(true))?;
        set_file_contents( &mut file, s.as_bytes())?;
    }

    println!("ok.");
    Ok(())
}