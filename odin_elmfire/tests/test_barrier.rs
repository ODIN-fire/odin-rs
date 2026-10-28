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
use odin_elmfire::Barrier;

/// what JSON.stringify(new Barrier(..)) produces in odin_elmfire.js (with main.GeoLineString and main.GeoLine values).
/// Note that JSON.stringify writes integer numbers without fraction
const JS_LINE_STRING: &str = r#"{"barrierType":"dozer-line","name":"line1","value":{"points":[{"lon":-121.9,"lat":37.1},{"lon":-121.85,"lat":37.15},{"lon":-121.8,"lat":37.2}]},"width":10}"#;
const JS_LINE: &str = r#"{"barrierType":"SEAT-drop","name":"drop1","value":{"start":{"lon":-121.9,"lat":37.1},"end":{"lon":-121,"lat":37}},"width":20}"#;

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
        for key in ["barrierType", "name"] {
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
