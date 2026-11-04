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

//! compatibility tests between odin_elmfire::Barrier and the odin_elmfire.js Barrier class

use odin_common::json_writer::{JsonWritable, JsonWriter};
use odin_elmfire::{Barrier, BarrierValue, ElmfireConfig, barrier::{MoistureDecay, MoistureIncrease, default_drop_agents}};

/// what JSON.stringify(new Barrier(..)) produces in odin_elmfire.js (with main.GeoLineString and main.GeoLine values).
/// Note that JSON.stringify writes integer numbers without fraction
const JS_LINE_STRING: &str = r#"{"barrierType":"dozer-line","name":"line1","value":{"points":[{"lon":-121.9,"lat":37.1},{"lon":-121.85,"lat":37.15},{"lon":-121.8,"lat":37.2}]},"width":10}"#;
const JS_LINE: &str = r#"{"barrierType":"LAT-drop","name":"drop1","value":{"start":{"lon":-121.9,"lat":37.1},"end":{"lon":-121,"lat":37}},"width":90,"step":2,"agent":"LTR"}"#;

fn to_json (b: &Barrier)->String {
    let mut w = JsonWriter::new();
    b.write_json_to( &mut w);
    w.to_string()
}

#[test]
fn test_js_rust_js () {
    for js in [JS_LINE_STRING, JS_LINE] {
        let b: Barrier = serde_json::from_str( js).unwrap_or_else( |e| panic!("failed to deserialize {js}: {e}"));
        let json = to_json( &b);
        println!("{js}\n  -> {json}");

        // compare with the original JS object (field order and number formatting differ)
        let js_value: serde_json::Value = serde_json::from_str( js).unwrap();
        let rust_value: serde_json::Value = serde_json::from_str( &json).unwrap();
        for key in ["barrierType", "name", "agent"] {
            assert_eq!( js_value[key], rust_value[key]);
        }
        assert_eq!( js_value["width"].as_f64(), rust_value["width"].as_f64()); // feet in both directions
        assert_eq!( js_value["value"].as_object().unwrap().keys().collect::<Vec<_>>(), rust_value["value"].as_object().unwrap().keys().collect::<Vec<_>>());

        // and back into Rust
        let b1: Barrier = serde_json::from_str( &json).unwrap();
        assert_eq!( json, to_json( &b1));
    }
}

#[test]
fn test_ron_roundtrip () {
    // barriers are persisted as part of the ElmFire RON data
    let barriers: Vec<Barrier> = vec![ serde_json::from_str( JS_LINE_STRING).unwrap(), serde_json::from_str( JS_LINE).unwrap() ];
    let ron = ron::to_string( &barriers).unwrap();
    println!("RON: {ron}");

    let barriers1: Vec<Barrier> = ron::from_str( &ron).unwrap_or_else( |e| panic!("failed to deserialize RON {ron}: {e}"));
    assert_eq!( barriers.len(), barriers1.len());
    for (b, b1) in barriers.iter().zip( barriers1.iter()) {
        assert_eq!( to_json(b), to_json(b1));
    }

    let empty: Vec<Barrier> = ron::from_str( "[]").unwrap();
    assert!( empty.is_empty());
}

#[test]
fn test_drop_fields () {
    let line: Barrier = serde_json::from_str( JS_LINE_STRING).unwrap(); // older/fuel break data without step and agent
    assert!( !line.is_air_drop());
    assert!( !to_json( &line).contains("agent"));

    let drop: Barrier = serde_json::from_str( JS_LINE).unwrap();
    assert!( drop.is_air_drop());
    let json: serde_json::Value = serde_json::from_str( &to_json( &drop)).unwrap();
    assert_eq!( json["step"], 2);
    assert_eq!( json["agent"], "LTR");
    assert_eq!( drop.geo_points().len(), 2);
}

#[test]
fn test_drop_agent_config () {
    // the sample config has to parse, and the configured agents have to be the defaults
    let config: ElmfireConfig = ron::from_str( &std::fs::read_to_string("configs/elmfire.ron").unwrap()).unwrap();
    assert_eq!( config.fuel_break_fbfm40, 99);
    let names: Vec<&str> = config.drop_agents.iter().map( |a| a.name.as_str()).collect();
    let default_names: Vec<String> = default_drop_agents().into_iter().map( |a| a.name).collect();
    assert_eq!( names, default_names);

    assert_eq!( MoistureIncrease::Set(60.0).delta( 5.0), 55.0);
    assert_eq!( MoistureIncrease::Set(60.0).delta( 70.0), 0.0); // already wetter
    assert_eq!( MoistureIncrease::Add(15.0).delta( 5.0), 15.0);

    assert_eq!( MoistureDecay::Constant.factor( 10.0), 1.0);
    assert_eq!( MoistureDecay::Linear(24.0).factor( 6.0), 0.75);
    assert_eq!( MoistureDecay::Linear(24.0).factor( 30.0), 0.0);
    assert_eq!( MoistureDecay::Exponential(0.5).factor( 1.0), 0.25);
}

/// a bucket drop as produced by odin_elmfire.js (main.js GeoEllipse value)
const JS_BUCKET: &str = r#"{"barrierType":"bucket-T2","name":"b1","value":{"lon":-121.9,"lat":37.1,"semiMajor":57,"semiMinor":21,"heading":45},"width":135,"step":3,"agent":"water"}"#;

#[test]
fn test_bucket_drop () {
    let b: Barrier = serde_json::from_str( JS_BUCKET).unwrap();
    assert!( b.is_air_drop());
    assert!( matches!( b.value(), BarrierValue::GeoEllipse(_)));
    assert_eq!( b.geo_points().len(), 33); // closed outline

    let json: serde_json::Value = serde_json::from_str( &to_json( &b)).unwrap();
    assert_eq!( json["value"]["semiMajor"], 57.0);
    assert_eq!( json["value"]["heading"], 45.0);

    // the untagged value enum must not confuse lines and ellipses
    let line: Barrier = serde_json::from_str( JS_LINE).unwrap();
    assert!( matches!( line.value(), BarrierValue::GeoLine(_)));

    let ron = ron::to_string( &b).unwrap();
    let b1: Barrier = ron::from_str( &ron).unwrap();
    assert_eq!( to_json( &b), to_json( &b1));
}
