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

 use uom::si::length::{meter, mile, nautical_mile};
 use odin_common::angle::*;
 use odin_common::geo::*;

// run with "cargo test test_basic -- --nocapture"

 #[test]
 fn test_basic() {
    let lon = Longitude::from_degrees(200.0);
    println!("display lon = {}", lon);
    println!("debug lon = {:?}", lon);

    let lon1 = Longitude::from_degrees(-160.0);
    println!("lon == lon1 : {}", lon == lon1);

    let input = r#"{ "longitude": -122.0, "latitude": 37.0 }"#;
    println!("input: '{}'", input);

    println!("\n-------------- GeoPoint");
    let p: GeoPoint = serde_json::from_str(&input).unwrap();
    println!("deserialized GeoPoint: {p:?}");

    // alternative deserialization format
    let input = r#"{ "lon": -122.0, "lat": 37.0 }"#;
    println!("alternative input: '{}' -> {}", input, serde_json::from_str::<GeoPoint>(&input).unwrap());

    let input = r#"{ "x": -122.0, "y": 37.0 }"#;
    println!("alternative input: '{}' -> {}", input, serde_json::from_str::<GeoPoint>(&input).unwrap());

    let ecef = p.as_ecef();
    println!("ECEF: {:?}", ecef);

    let s: String = serde_json::to_string(&p).unwrap();
    println!("serialized GeoPoint: '{}'", s);

    println!("\n-------------- GeoLine");
    let start = GeoPoint::from_lon_lat_degrees( -122.0, 33.0);
    let end = GeoPoint::from_lon_lat_degrees( -121.0, 37.0);
    let line = GeoLine::from_geo_points( start, end);
    println!("line: {:?}", line);

    let dist = line.haversine_distance();
    println!("haversine-distance: {}m", dist.get::<meter>());

    let dist = line.geodesic_distance();
    println!("geodesic-distance: {}m", dist.get::<meter>());

    let s: String = serde_json::to_string(&line).unwrap();
    println!("serialized GeoLine: '{}'", s);

    let line1: GeoLine = serde_json::from_str(&s).unwrap();
    println!("deserialized line1: {:?}", line1);

    let p = GeoPoint::from_lon_lat_degrees( -121.6, 35.0);
    let solution = line.closest_point(&p);
    println!("closest point on line: {:?}", solution);

    println!("\n-------------- GeoRect");
    let rect = GeoRect::from_wsen(
        Longitude::from_degrees(-122.0), Latitude::from_degrees(33.0),
        Longitude::from_degrees(-121.0), Latitude::from_degrees(36.0)
    );
    println!("rect: {:?}", rect);

    println!("area of rect: {:?}", rect.area());

    let s: String = serde_json::to_string(&rect).unwrap();
    println!("serialized GeoRect: '{}'", s);

    let rect1: GeoRect = serde_json::from_str(&s).unwrap();
    println!("deserialized rect1: {:?}", rect1);


    println!("\n-------------- GeoLineString");
    let points = vec![
        GeoPoint::from_lon_lat_degrees( -121.6, 35.0),
        GeoPoint::from_lon_lat_degrees( -121.7, 35.1),
        GeoPoint::from_lon_lat_degrees( -121.8, 35.2)
    ];
    let linestring = GeoLineString::from_geo_points( points);
    println!("line_string: {:?}", linestring);

    let s: String = serde_json::to_string(&linestring).unwrap();
    println!("serialized GeoLineString: '{}'", s);

    let linestring1: GeoLineString = serde_json::from_str(&s).unwrap();
    println!("deserialized linestring1: {:?}", linestring1);


    println!("\n-------------- GeoPolygon");
    let points = vec![
        GeoPoint::from_lon_lat_degrees( -122.0, 37.0),
        GeoPoint::from_lon_lat_degrees( -121.3, 36.5),
        GeoPoint::from_lon_lat_degrees( -121.0, 35.2),
        GeoPoint::from_lon_lat_degrees( -122.1, 35.0)
    ];
    let polygon = GeoPolygon::from_exterior_geo_points(points);
    println!("polygon: {:?}", polygon);

    let s: String = serde_json::to_string( &polygon).unwrap();
    println!("serialized GeoPolygon: '{}'", s);

    let polygon1: GeoPolygon = serde_json::from_str( &s).unwrap();
    println!("deserialize polygon1: {:?}", polygon1);

    let p = GeoPoint::from_lon_lat_degrees( -121.9, 36.9);
    println!("polygon contains {:?}: {}", p, polygon.contains(&p));

 }