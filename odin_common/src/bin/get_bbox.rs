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


use anyhow::{anyhow,Result};
use chrono::{Timelike, DateTime, Utc};
use uom::si::f64::{Length};
use uom::si::{length::{meter,kilometer,mile}};

use odin_common::{define_cli, geo::{GeoPoint,GeoRect}, utm::{UtmPoint,UtmRect, try_geo_to_utm}};

define_cli! { ARGS [about="get geo or utm bounding box around point"] =
    units:  String [help="length unit to use (km or mi)", long, default_value="km"],
    decimals: i32 [help="decimals to round to", short, long, default_value="3"],
    width:  f64 [help="width of bbox in selected units (km or mi)", long, default_value="30"],
    height: f64 [help="height of bbox in selected units (km or mi)", long, default_value="30"],
    utm:    bool [help="show bbox as UTM", long],
    lat:    f64 [help="center latitude [degrees]", allow_hyphen_values = true],
    lon:    f64 [help="center longitude [degrees]", allow_hyphen_values = true],
}


fn main ()->Result<()> {

    let geo_center = GeoPoint::from_lon_lat_degrees( ARGS.lon, ARGS.lat);
    let decimals = ARGS.decimals as usize;

    let (width,height) = if &ARGS.units == "km" {
        (Length::new::<kilometer>( ARGS.width), Length::new::<kilometer>( ARGS.height))
    } else if &ARGS.units == "mi" {
        (Length::new::<mile>( ARGS.width), Length::new::<mile>( ARGS.height))
    } else {
        return Err( anyhow!("no known units given (km or mi)"))
    };

    if ARGS.utm {
        let mut utm_center = try_geo_to_utm(&geo_center)?;
        let utm_rect = UtmRect::from_utm_point( &utm_center, width.get::<meter>(), height.get::<meter>());

        println!( "zone:  {: >15}", utm_rect.utm_zone.to_string());
        println!( "west:  {:15.decimals$}", utm_rect.west);
        println!( "south: {:15.decimals$}", utm_rect.south);
        println!( "east:  {:15.decimals$}", utm_rect.east);
        println!( "north: {:15.decimals$}", utm_rect.north);

    } else {
        let mut geo_rect = GeoRect::from_center( &geo_center, width, height);

        println!( "west:  {:10.decimals$}", geo_rect.west().degrees());
        println!( "south: {:10.decimals$}", geo_rect.south().degrees());
        println!( "east:  {:10.decimals$}", geo_rect.east().degrees());
        println!( "north: {:10.decimals$}", geo_rect.north().degrees());
    }

    Ok(())
}
