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
use uom::si::f64::{Length,Ratio};
use uom::si::{length::{meter,kilometer},ratio::{percent}};

use odin_common::{define_cli, geo::{GeoPoint,GeoRect}};
use odin_openmeteo::{self,OpenMeteoConfig};
use odin_landfire;
use odin_gdal::geods::GeoDs;
use odin_nfdrs::{
    CACHE_DIR, NfdrsConfig, NfdrsRecord, create_nfdrs_inputs, load_config, output_path, parse_nfdrs_records, run_nfdrs
};

define_cli! { ARGS [about="create input data files for NFDRS4"] =
    config: String [help="filename of NFDRS config file", long, default_value="nfdrs.ron"],
    wx_config: String [help="filename of OpenMeteo config file", long, default_value="openmeteo.ron"],
    wx_model: String [help="name of openmeteo wx model", long, default_value="gfs_hrrr"],
    lf_src: String [help="name of landfire source", long, default_value="lf_2025_conus.ron"],
    date:  DateTime<Utc> [help="simulation start date (default: now)", long, default_value_t=default_date()],
    dry_run: bool [help="only create input, don't execute NFDRS", long],
    show_result: bool [help="show computed output", long],
    forecast_days: u32 [help="number of forecast days to retrieve", long, default_value="1"],
    past_days: u32 [help="number of past days to retrieve", long, default_value="7"],
    lat:   f64 [help="center latitude [degrees]", allow_hyphen_values = true],
    lon:   f64 [help="center longitude [degrees]", allow_hyphen_values = true],
}

fn default_date()->DateTime<Utc> { Utc::now() }

#[tokio::main]
async fn main ()->Result<()> {
    let pos = GeoPoint::from_lon_lat_degrees( ARGS.lon, ARGS.lat);
    let config: NfdrsConfig = load_config( &ARGS.config)?;

    let openmeteo_config: OpenMeteoConfig = odin_openmeteo::load_config( &ARGS.wx_config)?;
    let lf_src: GeoDs = odin_landfire::load_config( &ARGS.lf_src)?;
    let forecast_days = ARGS.forecast_days;
    let past_days = ARGS.past_days;
    let wx_model = &ARGS.wx_model;
    let date = Utc::now();
    let output_path = output_path( &pos, &date, forecast_days, past_days, wx_model);

    // this reaches out to OpenMeteo and uses the Landfire GeoDs to obtain meteorological and fuel data for the given point
    let cfg_path = create_nfdrs_inputs( &openmeteo_config, &lf_src, &pos, &date, forecast_days, past_days, wx_model, &output_path).await?;
    println!("created NFDRS run configuration at {:?}", cfg_path);

    if !ARGS.dry_run {
        run_nfdrs( &config.nfdrs_cmd, &cfg_path).await?;

        if output_path.is_file() {
            println!("NFDRS4 created output at {:?}", output_path);

            if ARGS.show_result {
                let rs: Vec<NfdrsRecord> = parse_nfdrs_records( &output_path, |_| true)?;
                println!("{} records", rs.len());
                println!("datetime (UTC)             dfm-1h   dfm-10h  dfm-100h   lfm-h    lfm-w");
                println!("----------------------------------------------------------------------");
                for r in rs.iter() {
                    println!("{:25}  {:4.1}     {:4.1}     {:4.1}       {:4.1}     {:4.1}",
                        r.date.naive_local().format("%Y-%m-%d %H:%M"),
                        r.dfm_1h.get::<percent>(),
                        r.dfm_10h.get::<percent>(),
                        r.dfm_100h.get::<percent>(),
                        r.lfm_herb.get::<percent>(),
                        r.lfm_woody.get::<percent>(),
                    )
                }
            }

        } else {
            return Err( anyhow!("no output data") )
        }
    }

    Ok(())
}
