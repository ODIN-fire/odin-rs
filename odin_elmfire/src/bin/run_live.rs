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
    define_cli, run_captured_cmd,
    datetime::parse_utc_datetime,
    fs::path_as_str, geo::{GeoPoint,GeoRect},
    strings::parse_comma_separated,
    utm::{UtmPoint,UtmRect, try_geo_to_utm}
};
use odin_elmfire::{
    CACHE_DIR, load_config, ElmfireConfig, ElmFire, get_fixed_steps, get_utm_rect_around_point,
    errors::OdinElmfireError,
    input::{create_fems_input, create_landfire_input, create_wx_input}
};

define_cli! { ARGS [about="create live weather and fuel input files and run Elmfire"] =
    config: String [help="filename of Elmfire config file (refers to FEMS and landfire configs)", long, default_value="elmfire.ron"],
    fc_hours: u32 [help="number of forecast hours to compute", long, default_value="6"],
    dry_run: bool [help="dry run: create inputs but do not execute Elmfire", long],
    date:  DateTime<Utc> [help="simulation start date (default: now)", long, default_value_t=default_date(), value_parser=parse_utc_datetime],
    name: String [help="name of incident"],
    lat:   f64 [help="center latitude [degrees]", allow_hyphen_values = true],
    lon:   f64 [help="center longitude [degrees]", allow_hyphen_values = true],
}

fn default_date()->DateTime<Utc> { Utc::now() }

#[tokio::main]
async fn main ()->Result<()> {
    let config: ElmfireConfig = odin_elmfire::load_config( &ARGS.config)?;
    let geo_center = GeoPoint::from_lon_lat_degrees( ARGS.lon, ARGS.lat);
    let bbox_size = Length::new::<meter>( config.bbox_size_m as f64);
    let n_hours = ARGS.fc_hours;

    let mut ef = ElmFire::from_point_ignition( ARGS.name.clone(), ARGS.date, geo_center, bbox_size, n_hours, None, None)?;
    let data_file_path = ef.create_inputs( &config).await?;

    if !ARGS.dry_run {
        println!("running Elmfire on live {:?}..", data_file_path);
        let output = run_captured_cmd!( &config.elmfire_cmd, path_as_str( &data_file_path)?).await?;
        if output.status.success() {
            println!("done.");

            ef.process_outputs( &config)?;

        } else {
            eprintln!("Elmfire exit status: {}", output.status.code().unwrap_or(-1));
            eprintln!( "{}", String::from_utf8(output.stderr)?);
        }
    }

    Ok(())
}
