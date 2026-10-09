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
use chrono::{Timelike, DateTime, Utc};
use uom::si::f64::{Length};
use uom::si::{length::{meter,kilometer}};

use odin_common::{
    datetime::parse_utc_datetime, define_cli, fs::{file_contents_as_string, filepath_contents_as_string, path_as_str}, geo::{GeoPoint,GeoRect}, run_captured_cmd, strings::parse_comma_separated, utm::{UtmPoint,UtmRect, try_geo_to_utm}
};
use odin_elmfire::{
    CACHE_DIR, DATA_DIR, load_config, ElmfireConfig, ElmFire, fixed::get_fixed_steps, get_utm_rect_around_point,
    errors::OdinElmfireError,
    fixed::parse_file
};

define_cli! { ARGS [about="create live weather and fuel input files and run Elmfire"] =
    config: String [help="filename of Elmfire config file (refers to FEMS and landfire configs)", long, default_value="elmfire.ron"],
    fc_hours: u32 [help="number of forecast hours to compute", long, default_value="6"],
    dry_run: bool [help="dry run: create inputs but do not execute Elmfire", long],
    date:  DateTime<Utc> [help="simulation start date (default: now)", long, default_value_t=default_date(), value_parser=parse_utc_datetime],

    compute_wind: bool [help="compute micro-grid wind using WindNinja", long],
    spotting: Option<String> [help="filename of spotting settings to use", long],

    name: String [help="name of incident"],
    lat:   f64 [help="center latitude [degrees]", allow_hyphen_values = true],
    lon:   f64 [help="center longitude [degrees]", allow_hyphen_values = true],

    fixed: String [help="filename of fixed input *.nml to use"]
}

fn default_date()->DateTime<Utc> { Utc::now() }

#[tokio::main]
async fn main ()->Result<()> {
    let config: ElmfireConfig = odin_elmfire::load_config( &ARGS.config)?;
    let geo_center = GeoPoint::from_lon_lat_degrees( ARGS.lon, ARGS.lat);
    let bbox_size = Length::new::<meter>( config.bbox_size_m as f64);
    let n_hours = ARGS.fc_hours;

    let spotting = ARGS.spotting.as_ref().map(|p| filepath_contents_as_string(Path::new(p))).and_then(|res| res.ok());
    //let fixed_inputs = parse_file( &ARGS.fixed, n_hours as usize + 1)?;
    let fixed_src = filepath_contents_as_string( &ARGS.fixed)?;

    let mut ef = ElmFire::from_point_ignition(
        ARGS.name.clone(), ARGS.date, geo_center, bbox_size, n_hours, ARGS.compute_wind, Some(fixed_src), spotting, None)?;
    let data_file_path = ef.create_inputs( &config).await?;

    if !ARGS.dry_run {
        println!("running Elmfire on fixed {:?}..", data_file_path);
        let output = run_captured_cmd!( &config.elmfire_cmd, path_as_str( &data_file_path)?).await?;
        if output.status.success() {
            println!("done.");

            ef.process_outputs( &config)?;

        } else {
            eprintln!("Elmfire exit status: {}", output.status.code().unwrap_or(-1));
            eprintln!( "{}", String::from_utf8(output.stderr)?);
        }
    } else {
        println!("created Elmfire input file at {:?}", data_file_path);
    }

    Ok(())
}
