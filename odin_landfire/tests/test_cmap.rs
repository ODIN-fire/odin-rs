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

//! tests for landfire.gov CSV color maps and colorization

use odin_landfire::cmap::{ValueLut, colorize_alpha, odin_json_color_map, parse_rgb, parse_rgba, read_csv_color_map};

#[test]
fn test_csv_with_bom () {
    // LF2025_CBD.csv starts with a UTF-8 BOM and has a numeric (KGM3_X_100) and a descriptive (KGM3) name column
    let entries = read_csv_color_map( "resources/LF2025_CBD.csv").unwrap();
    assert_eq!( entries.len(), 3);

    assert!( entries[0].is_nodata());
    assert_eq!( entries[0].value, -9999);

    assert_eq!( entries[2].value, 1);
    assert_eq!( entries[2].name, "CBD = 0.01 kg/m^3"); // from KGM3 column
    assert_eq!( entries[2].rgb, [190,255,232]);
}

#[test]
fn test_evt_lut () {
    let entries = read_csv_color_map( "resources/LF2025_EVT.csv").unwrap();
    assert!( entries.len() > 1000);
    let lut = ValueLut::from_entries( &entries);

    let e = entries.iter().find( |e| e.value == 4401).unwrap();
    assert_eq!( e.name, "Alaska Arctic Coastal Sedge-Dwarf-Shrubland");
    assert_eq!( lut.rgba(4401), [255,205,112,255]);

    assert_eq!( lut.rgba(-9999), [0,0,0,0]); // Fill-NoData is transparent
    assert_eq!( lut.rgba(32767), [0,0,0,0]); // so are values that are not in the color map (e.g. GeoTIFF nodata)

    let rgba = lut.colorize( &[4401, -9999], &[], [0,0,0,0]);
    assert_eq!( rgba, vec![255,205,112,255, 0,0,0,0]);

    // nodata values (e.g. declared 32767 and Fill-NoData -9999) are rendered with the nodata color
    let rgba = lut.colorize( &[4401, -9999, 32767], &[-9999, 32767], [255,0,255,128]);
    assert_eq!( rgba, vec![255,205,112,255, 255,0,255,128, 255,0,255,128]);

    let json: serde_json::Value = serde_json::from_str( &odin_json_color_map( &entries)).unwrap();
    let items = json.as_array().unwrap();
    assert_eq!( items.len(), entries.len() - 1); // no nodata entry
    assert_eq!( items[0]["code"], "4401");
    assert_eq!( items[0]["color"], "#ffcd70");
}

#[test]
fn test_alpha () {
    let rgba = colorize_alpha( &[-9999, -1, 0, 30, 60, 90], &[-9999], [0,0,0,0], [255,0,0], 0.0, 60.0);
    let alphas: Vec<u8> = rgba.chunks(4).map( |c| c[3]).collect();
    assert_eq!( alphas, vec![0, 0, 0, 128, 255, 255]);
    assert!( rgba.chunks(4).skip(1).all( |c| c[0..3] == [255,0,0]));

    // zero is a valid value (transparent since <= min), nodata values get the nodata color
    let rgba = colorize_alpha( &[-9999, 0, 32767, 60], &[-9999, 32767], [255,0,255,128], [0,160,0], 0.0, 60.0);
    assert_eq!( rgba, vec![255,0,255,128, 0,160,0,0, 255,0,255,128, 0,160,0,255]);
}

#[test]
fn test_parse_rgb () {
    assert_eq!( parse_rgb("ff8000"), Some([255,128,0]));
    assert_eq!( parse_rgb("#00ff00"), Some([0,255,0]));
    assert_eq!( parse_rgb("fff"), None);

    assert_eq!( parse_rgba("ff00ff80"), Some([255,0,255,128]));
    assert_eq!( parse_rgba("#ff8000"), Some([255,128,0,255])); // RRGGBB is opaque
}
