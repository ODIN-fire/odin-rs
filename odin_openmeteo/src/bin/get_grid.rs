/*
 * Copyright © 2026, United States Government, as represented by the Administrator of
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

use std::{path::{Path,PathBuf}, str::FromStr};
use anyhow::{Result,anyhow};
use ndarray::prelude::*;
use tokio;
use chrono::{DateTime,Utc};
use uom::si::{f64::Length};
use uom::si::{length::{meter,kilometer}, velocity::{mile_per_hour}, angle::{degree}};

use odin_common::{
    define_cli,
    datetime::{hours},
    geo::GeoPoint, utm::{GeoRectMapping, OdinUtmError, UtmPoint, UtmRect, UtmZone, geo_to_utm_zone, utm_to_geo}
};
use odin_openmeteo::{DataSet, OpenMeteoConfig, dataset::WindData10m, get_fc_days, grid::{GridAlgorithm, UtmGrid}, ids::ModelId, load_config};

define_cli! { ARGS [about="retrieve 1h wind forecasts for a given bbox and store as UTM datasets"] =
    config: String [help="filename of OpenMeteo config file", long, default_value="openmeteo.ron"],
    model: String [help="wx model to use", long, default_value="ncep_hrrr_conus"],
    hours:  u64 [help="number of 1h time steps to retrieve", long, default_value="6"],
    n2:  usize [help="number of rows,columns from center row,column (total number 2 * n2 + 1)", long, default_value="5"],
    alg: String [help="gridding algorithm (nearest,idw)", long, default_value="idw"],
    file_path: Option<String> [help="optional file path to store/read Open-Meteo query response (if exists no external query will be made)", long],
    res: f64 [help="pixel resolution in meter", long, default_value="3000"],
    lon: f64 [help="center longitude in degrees", allow_hyphen_values = true],
    lat: f64 [help="center latitude in degrees", allow_hyphen_values = true]
}

#[tokio::main]
async fn main ()->Result<()> {
    let config: OpenMeteoConfig = load_config( &ARGS.config)?;
    let geo_center = GeoPoint::from_lon_lat_degrees( ARGS.lon, ARGS.lat);
    let cell_size = Length::new::<meter>(ARGS.res);
    let start_date = Utc::now();
    let end_date = start_date + hours(ARGS.hours);
    let n2 = ARGS.n2;
    let model = ModelId::from_str( &ARGS.model)?;
    let alg = if &ARGS.alg == "nearest" { GridAlgorithm::NearestNeighbor } else { GridAlgorithm::idw( 1.5, 2.0) };

    let mut utm_grid: UtmGrid<WindData10m> = UtmGrid::from_center( &geo_center, cell_size, n2, n2)?;

    if let Some(path) = ARGS.file_path.as_ref().map(|s| Path::new(s)) {
        utm_grid.get_cached_hourly_data( &config, model, &start_date, &end_date, alg, path).await?;
    } else {
        utm_grid.get_hourly_data( &config, model, &start_date, &end_date, alg).await?;
    }

    //utm_grid.dump_data(|d| if let Some(d) = d { format!("{:3.0}", d.wind_direction_10m[0].get::<degree>()) } else { "   ".to_string() });

    let t0 = utm_grid.time_step_data(0, |opt_v,i| { if let Some(v) = opt_v { v.wind_direction_10m[i].get::<degree>() } else { 999.0 } })?;
    println!("{:3.0}", t0);
    println!();

    let t0 = utm_grid.time_step_data(0, |opt_v,i| { if let Some(v) = opt_v { v.wind_speed_10m[i].get::<mile_per_hour>() } else { 999.0 } })?;
    println!("{:3.0}", t0);


    //println!("[3,3] = {:3.0}", &utm_grid.data[(3,3)].as_ref().unwrap().wind_direction_10m[0].get::<degree>());
    //println!("[4,3] = {:3.0}", &utm_grid.data[(4,3)].as_ref().unwrap().wind_direction_10m[0].get::<degree>());

    Ok(())
}
