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

use anyhow::{Result,anyhow};
use tokio;
use odin_common::{datetime::minutes, define_cli, geo::{GeoPoint,GeoRect}, utm::{UtmPoint,geo_to_utm,try_geo_to_utm_zone,get_xy_vecs}};
use odin_openmeteo::{load_config,OpenMeteoLocationData,dataset::MinimalSfcData};

define_cli! { ARGS [about="retrieve 15min forecasts"] =
    config: String [help="filename of OpenMeteo config file", long, default_value="openmeteo.ron"],
    model: String [help="wx model to use", long, default_value="gfs_hrrr"],
    steps:  usize [help="number of 15min time steps to retrieve", long, default_value="24"],
    west:   f64 [help="west [degrees]", allow_hyphen_values = true],
    south:  f64 [help="south [degrees]", allow_hyphen_values = true],
    east:   f64 [help="east [degrees]", allow_hyphen_values = true],
    north:  f64 [help="north [degrees]", allow_hyphen_values = true],
}


#[tokio::main]
async fn main ()->Result<()> {
    let config = load_config( &ARGS.config)?;
    let bbox = GeoRect::from_wsen_degrees( ARGS.west, ARGS.south, ARGS.east, ARGS.north);
    let dur = minutes( (ARGS.steps * 15) as u64);

    let vs: Vec<OpenMeteoLocationData<MinimalSfcData>> = OpenMeteoLocationData::get_minutely_bbox_forecasts( &config, &bbox, dur, &ARGS.model).await?;
    let vs_len = vs.len();
    println!("retrieved {} location data records", vs_len);


    let center = bbox.center();
    let utm_center: UtmPoint = center.try_into()?;
    let utm_zone = utm_center.utm_zone;
    let gps = OpenMeteoLocationData::geo_points(&vs);
    let utm_pos: Vec<UtmPoint> = UtmPoint::from_geo_points_in_zone( &gps, utm_zone)?;
    let (xs,ys) = get_xy_vecs( &utm_pos, utm_zone).ok_or( anyhow!("incompatible UTM station coordinates"))?;

    if !vs.is_empty() {
        if let Some(time_steps) = OpenMeteoLocationData::minutely_times(&vs[0]) {
            for i in 0..time_steps.len() {
                let mut ws: Vec<f64> = Vec::with_capacity( vs_len);
                let mut wd: Vec<f64> = Vec::with_capacity( vs_len);

                for ld in vs.iter() {
                    if let Some(md) = &ld.minutely_data {
                        ws.push( md.minutely_15.wind_speed_10m[i] as f64);
                        ws.push( md.minutely_15.wind_speed_10m[i] as f64);
                    }
                }
            }
        }
    }

    Ok(())
}
