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

use std::str::FromStr;
use uom::si::{f64::Length, length::{inch,millimeter}};
use tokio;
use chrono::{DateTime,Utc};
use anyhow::{Result,anyhow};

use odin_common::{define_cli, datetime::{days,get_tz_of_location}, geo::GeoPoint};
use odin_openmeteo::ids::ModelId;
use odin_nfdrs::{Fw21Record, get_fw21_records};

define_cli! { ARGS [about="retrieve 15min forecasts"] =
   config: String [help="filename of OpenMeteo config file", long, default_value="openmeteo.ron"],
   days: u64 [help="number of forecast days to retrieve", long, default_value="1"],
   past_days: u64 [help="number of past days to retrieve", long, default_value="7"],
   model: String [help="wx model to use", long, default_value="ncep_hrrr"],
   fw21_path: Option<String> [help="path to save FW21 csv to", short,long],
   latitude: f64  [help="latitude [degrees)", allow_hyphen_values = true],
   longitude: f64 [help="longitude [degrees]", allow_hyphen_values = true]
}

#[tokio::main]
async fn main ()->Result<()> {
    let config = odin_openmeteo::load_config( &ARGS.config)?;

    let pos = GeoPoint::from_lon_lat_degrees( ARGS.longitude, ARGS.latitude);
    let now = Utc::now();
    let start = now - days(ARGS.past_days);
    let end = now + days( ARGS.days);
    let model = ModelId::from_str( &ARGS.model)?;

    let recs = get_fw21_records( &config, &pos, &start, &end, model).await?;

    if let Some(path) = &ARGS.fw21_path {
        if let Some(tz) = get_tz_of_location( &pos) {
            Fw21Record::save_as_csv( path, &recs, &tz)?;

        } else {
            return Err( anyhow!( "no valid timezone for position") )
        }
    }

    Ok(())
}
