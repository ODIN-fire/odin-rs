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

//! tests for JSON serialization of odin_common::geo types in both directions:
//!  - Rust -> Javascript: JsonWritable output has to match the odin_server/assets/main.js GeoX classes
//!  - Javascript -> Rust: JSON produced by main.js GeoX objects (e.g. JSON.stringify) has to deserialize into geo types

use serde::de::DeserializeOwned;
use uom::si::{f64::Length, length::meter};
use odin_common::{
    angle::{Latitude, Longitude},
    geo::{GeoCircle, GeoEllipse, GeoLine, GeoLineString, GeoPoint, GeoPoint3, GeoPolygon, GeoRect},
    json_writer::{JsonWritable, JsonWriter}
};

fn to_json<T: JsonWritable> (v: &T)->String {
    let mut w = JsonWriter::new();
    v.write_json_to( &mut w);
    w.to_string()
}

/// Rust -> JSON -> Rust -> JSON has to produce the same JSON
fn check_roundtrip<T: JsonWritable + DeserializeOwned> (v: &T)->String {
    let json = to_json(v);
    let v1: T = serde_json::from_str( &json).unwrap_or_else( |e| panic!("failed to deserialize {json}: {e}"));
    assert_eq!( json, to_json( &v1));
    println!("{json}");
    json
}

fn pt (lon: f64, lat: f64)->GeoPoint { GeoPoint::from_lon_lat_degrees( lon, lat) }

#[test]
fn test_rust_to_js () {
    let json = check_roundtrip( &pt(-121.5, 37.25));
    assert_eq!( json, r#"{"lon":-121.50000,"lat":37.25000}"#);

    check_roundtrip( &GeoPoint3::from_lon_lat_degrees_alt_meters( -121.5, 37.25, 123.0));

    let json = check_roundtrip( &GeoLine::from_geo_points( pt(-121.5, 37.25), pt(-121.0, 37.5)));
    assert_eq!( json, r#"{"start":{"lon":-121.50000,"lat":37.25000},"end":{"lon":-121.00000,"lat":37.50000}}"#);

    let json = check_roundtrip( &GeoLineString::from_geo_points( vec![pt(-121.5, 37.25), pt(-121.0, 37.5), pt(-120.5, 37.0)]));
    assert!( json.starts_with( r#"{"points":[{"lon":-121.50000,"lat":37.25000},"#));

    let json = check_roundtrip( &GeoRect::from_wsen_degrees( -122.0, 37.0, -121.0, 38.0));
    assert!( json.contains( r#""west":-122"#) && json.contains( r#""north":38"#));

    let json = check_roundtrip( &GeoCircle::new( Longitude::from_degrees(-121.5), Latitude::from_degrees(37.25), Length::new::<meter>(1500.0)));
    assert_eq!( json, r#"{"lon":-121.50000,"lat":37.25000,"radius":1500.0}"#);

    let ext = vec![ pt(-122.0,37.0), pt(-121.0,37.0), pt(-121.0,38.0), pt(-122.0,38.0), pt(-122.0,37.0) ];
    let json = check_roundtrip( &GeoPolygon::from_exterior_geo_points( ext.clone()));
    assert!( json.starts_with( r#"{"exterior":["#) && !json.contains( "interiors"));

    let hole = vec![ pt(-121.6,37.4), pt(-121.4,37.4), pt(-121.4,37.6), pt(-121.6,37.6), pt(-121.6,37.4) ];
    let json = check_roundtrip( &GeoPolygon::from_geo_points( ext, vec![hole]));
    assert!( json.contains( r#""interiors":[[{"lon":-121.60000,"lat":37.40000},"#));
}

#[test]
fn test_js_to_rust () {
    // the main.js GeoX.template strings
    let _: GeoPoint = serde_json::from_str( r#"{ "lon": 0.0, "lat": 0.0 }"#).unwrap();
    let _: GeoPoint3 = serde_json::from_str( r#"{ "lon": 0.0, "lat": 0.0, "alt": 0 }"#).unwrap();
    let _: GeoLine = serde_json::from_str( "{\n  \"start\": {\"lon\": 0.0, \"lat\": 0.0},\n  \"end\": {\"lon\": 0.0, \"lat\": 0.0}\n}").unwrap();
    let _: GeoLineString = serde_json::from_str( "{\n  \"points\": [\n    {\"lon\": 0.0, \"lat\": 0.0},\n    {\"lon\": 0.0, \"lat\": 0.0},\n    {\"lon\": 0.0, \"lat\": 0.0}\n  ]\n}").unwrap();
    let _: GeoPolygon = serde_json::from_str( "{\n  \"exterior\": [\n    {\"lon\": 0.0, \"lat\": 0.0},\n    {\"lon\": 0.0, \"lat\": 0.0},\n    {\"lon\": 0.0, \"lat\": 0.0}\n  ]\n}").unwrap();
    let _: GeoRect = serde_json::from_str( "{\n  \"west\": 0.0,\n  \"south\": 0.0,\n  \"east\": 0.0,\n  \"north\": 0.0\n}").unwrap();
    let _: GeoCircle = serde_json::from_str( "{\n  \"lon\": 0.0,\n  \"lat\": 0.0,\n  \"radius\": 0.0\n}").unwrap();

    // JSON.stringify() output - note that integer values are written without fraction
    let rect: GeoRect = serde_json::from_str( r#"{"west":-122,"south":37,"east":-121.5,"north":38}"#).unwrap();
    assert_eq!( rect.west().degrees(), -122.0);
    assert_eq!( rect.north().degrees(), 38.0);

    let circle: GeoCircle = serde_json::from_str( r#"{"lon":-121,"lat":37,"radius":1000}"#).unwrap();
    assert_eq!( circle.lon().degrees(), -121.0);
    assert_eq!( circle.radius().get::<meter>(), 1000.0);

    let p: GeoPoint = serde_json::from_str( r#"{"lon":-121,"lat":37}"#).unwrap();
    assert_eq!( p.lon_lat_degrees(), (-121.0, 37.0));

    let line: GeoLine = serde_json::from_str( r#"{"start":{"lon":-121.5,"lat":37.25},"end":{"lon":-121,"lat":37.5}}"#).unwrap();
    assert_eq!( line.end().lon_lat_degrees(), (-121.0, 37.5));

    let ls: GeoLineString = serde_json::from_str( r#"{"points":[{"lon":-121.5,"lat":37.25},{"lon":-121,"lat":37.5}]}"#).unwrap();
    assert_eq!( ls.coords_count(), 2);

    let poly: GeoPolygon = serde_json::from_str(
        r#"{"exterior":[{"lon":-122,"lat":37},{"lon":-121,"lat":37},{"lon":-121,"lat":38},{"lon":-122,"lat":37}],
            "interiors":[[{"lon":-121.6,"lat":37.4},{"lon":-121.4,"lat":37.4},{"lon":-121.4,"lat":37.6},{"lon":-121.6,"lat":37.4}]]}"#).unwrap();
    assert!( poly.has_interiors());
    assert_eq!( poly.as_interior_geo_points()[0].len(), 4);

    let p3: GeoPoint3 = serde_json::from_str( r#"{"lon":-121.5,"lat":37.25,"alt":120}"#).unwrap();
    assert_eq!( p3.altitude_meters(), 120.0);
}

#[test]
fn test_ellipse () {
    use odin_common::angle::Angle360;

    let e = GeoEllipse::new( Longitude::from_degrees(-121.5), Latitude::from_degrees(37.25), Length::new::<meter>(100.0), Length::new::<meter>(40.0), Angle360::from_degrees(90.0));
    let json = check_roundtrip( &e);
    assert_eq!( json, r#"{"lon":-121.50000,"lat":37.25000,"semiMajor":100.0,"semiMinor":40.0,"heading":90.0}"#);

    // what main.js GeoEllipse produces
    let e1: GeoEllipse = serde_json::from_str( r#"{"lon":-121.5,"lat":37.25,"semiMajor":100,"semiMinor":40,"heading":90}"#).unwrap();
    assert_eq!( to_json( &e1), json);

    // heading 90 means the major axis points east, i.e. the first outline point is 100m east of the center
    let ps = e.outline_points( 4);
    assert_eq!( ps.len(), 5);
    assert_eq!( ps[0], ps[4]);
    assert!( ps[0].longitude_degrees() > -121.5 && (ps[0].latitude_degrees() - 37.25).abs() < 1e-9);
    assert!( (ps[1].latitude_degrees() - 37.25) < 0.0); // minor axis direction for heading 90 is south
    assert!( (e.area().value - std::f64::consts::PI * 4000.0).abs() < 1e-6);
}
