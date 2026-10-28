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

//! tests for FixedInput and its compatibility with the odin_elmfire.js fixed input representation

use uom::si::{ratio::percent, velocity::{mile_per_hour, meter_per_second}, thermodynamic_temperature::degree_fahrenheit};
use odin_common::json_writer::{JsonWritable, JsonWriter};
use odin_elmfire::{FixedInput, fixed::parse_fixed_wx_lines};

/// what odin_elmfire.js getFixed() produces (JSON.stringify writes integers without fraction)
const JS_FIXED: &str = r#"{"ws":[20,25,30],"wd":[315,300],"m1":[2],"m10":[3],"m100":[5],"lh":[30],"lw":[60],"t2":[80,85],"tcc":[10]}"#;

fn to_json (fixed: &FixedInput)->String {
    let mut w = JsonWriter::new();
    fixed.write_json_to( &mut w);
    w.to_string()
}

#[test]
fn test_js_to_rust () {
    let fixed: FixedInput = serde_json::from_str( JS_FIXED).unwrap();
    fixed.check().unwrap();

    assert_eq!( fixed.ws.len(), 3);
    assert!( (fixed.ws[0].get::<mile_per_hour>() - 20.0).abs() < 1e-9);
    assert!( (fixed.ws[0].get::<meter_per_second>() - 8.9408).abs() < 1e-4); // unit conversion for WindNinja
    assert_eq!( fixed.wd[1].degrees(), 300.0);
    assert!( (fixed.m1[0].get::<percent>() - 2.0).abs() < 1e-9);
    assert!( (fixed.t2[1].get::<degree_fahrenheit>() - 85.0).abs() < 1e-9);

    // hourly values are padded with the last value
    let n_steps = 5;
    assert_eq!( FixedInput::hourly_values( &fixed.ws, n_steps, |v| v.get::<mile_per_hour>()).iter().map(|v| v.round()).collect::<Vec<f64>>(), vec![20.0, 25.0, 30.0, 30.0, 30.0]);
    assert_eq!( FixedInput::hourly_values( &fixed.m1, n_steps, |v| v.get::<percent>()).iter().map(|v| v.round()).collect::<Vec<f64>>(), vec![2.0; 5]);
    assert_eq!( FixedInput::value_at( &fixed.wd, 7).map(|v| v.degrees()), Some(300.0));
}

#[test]
fn test_rust_to_js () {
    let fixed: FixedInput = serde_json::from_str( JS_FIXED).unwrap();
    let json = to_json( &fixed);
    println!("{json}");

    // same values in UI units as sent by the client
    let js: serde_json::Value = serde_json::from_str( JS_FIXED).unwrap();
    let rs: serde_json::Value = serde_json::from_str( &json).unwrap();
    for key in ["ws", "wd", "m1", "m10", "m100", "lh", "lw", "t2", "tcc"] {
        let js_vs: Vec<f64> = js[key].as_array().unwrap().iter().map( |v| v.as_f64().unwrap()).collect();
        let rs_vs: Vec<f64> = rs[key].as_array().unwrap().iter().map( |v| v.as_f64().unwrap()).collect();
        assert_eq!( js_vs, rs_vs, "{key}");
    }

    // and back
    let fixed1: FixedInput = serde_json::from_str( &json).unwrap();
    assert_eq!( json, to_json( &fixed1));
}

#[test]
fn test_ron_roundtrip () { // ElmFire (and hence FixedInput) is persisted as RON
    let fixed: FixedInput = serde_json::from_str( JS_FIXED).unwrap();
    let ron = ron::to_string( &fixed).unwrap();
    println!("{ron}");
    let fixed1: FixedInput = ron::from_str( &ron).unwrap();
    assert_eq!( to_json( &fixed), to_json( &fixed1));
}

#[test]
fn test_optional_and_missing () {
    // tcc and t2 are optional
    let fixed: FixedInput = serde_json::from_str( r#"{"ws":[20],"wd":[315],"m1":[2],"m10":[3],"m100":[5],"lh":[30],"lw":[60]}"#).unwrap();
    fixed.check().unwrap();
    assert!( fixed.t2.is_empty() && fixed.tcc.is_empty());
    assert!( FixedInput::value_at( &fixed.t2, 0).is_none());

    // mandatory values have to be non-empty
    let fixed: FixedInput = serde_json::from_str( r#"{"ws":[],"wd":[315],"m1":[2],"m10":[],"m100":[5],"lh":[30],"lw":[60]}"#).unwrap();
    let err = fixed.check().unwrap_err().to_string();
    println!("{err}");
    assert!( err.contains("ws") && err.contains("m10"));
}

#[test]
fn test_nml () {
    let src = "! fixed input\nws = 20, 25\nwd = 315\nm1 = 2\nm10 = 3\nm100 = 5\nlh = 30\nlw = 60\nt2 = 80\n";
    let fixed = parse_fixed_wx_lines( src).unwrap();
    assert_eq!( fixed.ws.len(), 2);
    assert!( fixed.tcc.is_empty());

    assert!( parse_fixed_wx_lines( "ws = 20\nwd = 315\n").is_err()); // missing fuel moisture
}
