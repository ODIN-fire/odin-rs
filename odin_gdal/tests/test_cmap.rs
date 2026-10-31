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

//! tests for pixel buffer colorization (odin_gdal::cmap)

use odin_gdal::cmap::{CmapEntry, ValueLut, colorize_alpha, json_legend, parse_rgb, parse_rgba};

fn entry (value: i32, rgb: [u8;3], is_nodata: bool)->CmapEntry {
    CmapEntry { value, name: format!("v{value}"), rgb, is_nodata }
}

#[test]
fn test_lut () {
    let entries = vec![ entry(-9999, [255,255,255], true), entry(0, [10,20,30], false), entry(200, [1,2,3], false) ];

    // Int16 data
    let lut = ValueLut::new( &entries, i16::MIN as i32);
    assert_eq!( lut.rgba(0), [10,20,30,255]);
    assert_eq!( lut.rgba(-9999), [0,0,0,0]); // nodata entries are transparent
    assert_eq!( lut.rgba(5), [0,0,0,0]);     // so are values that are not in the color map

    let rgba = lut.colorize( &[0.0, -9999.0, 32767.0, 200.0], &[-9999.0, 32767.0], [255,0,255,128]);
    assert_eq!( rgba, vec![10,20,30,255, 255,0,255,128, 255,0,255,128, 1,2,3,255]);

    // UInt8 data (negative values can't be represented)
    let lut = ValueLut::new( &entries, 0);
    assert_eq!( lut.rgba(200), [1,2,3,255]);
    assert_eq!( lut.rgba(-1), [0,0,0,0]);

    let json: serde_json::Value = serde_json::from_str( &json_legend( &entries)).unwrap();
    assert_eq!( json.as_array().unwrap().len(), 2); // no nodata entry
    assert_eq!( json[0]["color"], "#0a141e");
}

#[test]
fn test_alpha () {
    let rgba = colorize_alpha( &[-9999.0, -1.0, 0.0, 30.0, 60.0, 90.0], &[-9999.0], [0,0,0,0], [255,0,0], 0.0, 60.0);
    let alphas: Vec<u8> = rgba.chunks(4).map( |c| c[3]).collect();
    assert_eq!( alphas, vec![0, 0, 0, 128, 255, 255]);
    assert!( rgba.chunks(4).skip(1).all( |c| c[0..3] == [255,0,0]));

    // zero is a valid value (transparent since <= min), nodata values get the nodata color
    let rgba = colorize_alpha( &[-9999.0, 0.0, 32767.0, 60.0], &[-9999.0, 32767.0], [255,0,255,128], [0,160,0], 0.0, 60.0);
    assert_eq!( rgba, vec![255,0,255,128, 0,160,0,0, 255,0,255,128, 0,160,0,255]);
}

#[test]
fn test_alpha_float () {
    // float data sets: NaN is always nodata, declared float nodata values are compared exactly
    let nodata = f32::MIN as f64;
    let rgba = colorize_alpha( &[f64::NAN, nodata, 0.25, 0.5], &[nodata], [9,9,9,9], [0,0,255], 0.0, 1.0);
    assert_eq!( rgba, vec![9,9,9,9, 9,9,9,9, 0,0,255,64, 0,0,255,128]);
}

#[test]
fn test_parse_colors () {
    assert_eq!( parse_rgb("ff8000"), Some([255,128,0]));
    assert_eq!( parse_rgb("#00ff00"), Some([0,255,0]));
    assert_eq!( parse_rgb("fff"), None);

    assert_eq!( parse_rgba("ff00ff80"), Some([255,0,255,128]));
    assert_eq!( parse_rgba("#ff8000"), Some([255,128,0,255])); // RRGGBB is opaque
}
