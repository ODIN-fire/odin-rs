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

use uom::si::{f64::Length, length::{inch,millimeter}};
use tokio;
use chrono::{DateTime,Utc};
use anyhow::{Result,anyhow};

use odin_common::{define_cli, datetime::days, geo::GeoPoint};
use odin_openmeteo::queries::get_annual_precipitation;

define_cli! { ARGS [about="retrieve 15min forecasts"] =
   config: String [help="filename of OpenMeteo config file", long, default_value="openmeteo.ron"],
   latitude: f64  [help="latitude [degrees)", allow_hyphen_values = true],
   longitude: f64 [help="longitude [degrees]", allow_hyphen_values = true]
}

#[tokio::main]
async fn main ()->Result<()> {
    let config = odin_openmeteo::load_config( &ARGS.config)?;

    let pos = GeoPoint::from_lon_lat_degrees( ARGS.longitude, ARGS.latitude);
    let end_date = Utc::now() - days(1);
    let avg_precip = get_annual_precipitation( &config, &pos, &end_date).await?;

    println!("average annual precipitation: {:.1} [in]", avg_precip.get::<inch>());

    Ok(())
}
