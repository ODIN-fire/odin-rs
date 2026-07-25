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

use odin_common::angle::*;
use odin_common::geo::*;
use odin_common::utm::*;

 // run with "cargo test --test test_utm test_rect_conversion -- --nocapture"
#[test]
fn test_rect_conversion () {
    let geo_rect = GeoRect::from_wsen_degrees( -122.68, 36.99, -121.86, 37.48);
    println!("geo: {:?}", geo_rect);

    let mut r_ext = UtmRect::from_geo_rect( &geo_rect, GeoRectMapping::ExteriorGeo).unwrap();
    r_ext.round();
    println!("utm ext_geo: {:?}", r_ext);

    let mut r_med = UtmRect::from_geo_rect( &geo_rect, GeoRectMapping::Median).unwrap();
    r_med.round();
    println!("utm median:  {:?}", r_med);
    assert!((r_med.west - 528476.11).abs() < 100.0);
    assert!((r_med.south - 4093810.88).abs() < 1.0);

    let mut r_int = UtmRect::from_geo_rect( &geo_rect, GeoRectMapping::InteriorGeo).unwrap();
    r_int.round();
    println!("utm int_geo: {:?}", r_int);

    assert!((r_ext.west > r_med.west) && (r_med.west > r_int.west));

    let gr_med: GeoRect = r_med.to_geo_rect( GeoRectMapping::Median);
    println!("geo (med): {:?}", gr_med);

    assert!( (geo_rect.west().degrees() - gr_med.west().degrees()).abs() < 0.05);
    assert!( (geo_rect.north().degrees() - gr_med.north().degrees()).abs() < 0.05);
    assert!( (geo_rect.east().degrees() - gr_med.east().degrees()).abs() < 0.05);
    assert!( (geo_rect.south().degrees() - gr_med.south().degrees()).abs() < 0.05);
}

#[test]
fn test_point_conversion () {
    let geo_point = GeoPoint::from_lon_lat_degrees( -122.123, 37.234);
    println!("geo: {:?}", geo_point);
    let utm_point: UtmPoint = geo_point.try_into().unwrap();
    println!("utm: {:?}", utm_point);

    assert!( utm_point.easting.round() == 577793.0);
    assert!( utm_point.northing.round() == 4121192.0);

    let gp: GeoPoint = utm_point.into();
    println!(" -> geo: {:?}", gp);
    assert!( (gp.latitude_degrees() - geo_point.latitude_degrees()).abs() < 0.05);
    assert!( (gp.longitude_degrees() - geo_point.longitude_degrees()).abs() < 0.05);
}
