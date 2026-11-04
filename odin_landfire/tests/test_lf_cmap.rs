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

//! tests for landfire.gov CSV color maps

use odin_gdal::cmap::{ValueLut, json_legend};
use odin_landfire::lf_cmap::{read_csv_color_map, read_lf_cmap};

#[test]
fn test_csv_with_bom () {
    // LF2025_CBD.csv starts with a UTF-8 BOM and has a numeric (KGM3_X_100) and a descriptive (KGM3) name column
    let entries = read_csv_color_map( "resources/LF2025_CBD.csv").unwrap();
    assert_eq!( entries.len(), 3);

    assert!( entries[0].is_nodata); // "Fill-NoData"
    assert_eq!( entries[0].value, -9999);

    assert_eq!( entries[2].value, 1);
    assert_eq!( entries[2].name, "CBD = 0.01 kg/m^3"); // from KGM3 column
    assert_eq!( entries[2].rgb, [190,255,232]);
}

#[test]
fn test_evt_lut () {
    let entries = read_csv_color_map( "resources/LF2025_EVT.csv").unwrap();
    assert!( entries.len() > 1000);
    let lut = ValueLut::new( &entries, i16::MIN as i32); // EVT is Int16

    let e = entries.iter().find( |e| e.value == 4401).unwrap();
    assert_eq!( e.name, "Alaska Arctic Coastal Sedge-Dwarf-Shrubland");
    assert_eq!( lut.rgba(4401), [255,205,112,255]);

    assert_eq!( lut.rgba(-9999), [0,0,0,0]); // Fill-NoData is transparent
    assert_eq!( lut.rgba(32767), [0,0,0,0]); // so are values that are not in the color map (e.g. GeoTIFF nodata)

    let rgba = lut.colorize( &[4401.0, -9999.0], &[], [0,0,0,0]);
    assert_eq!( rgba, vec![255,205,112,255, 0,0,0,0]);

    // nodata values (e.g. declared 32767 and Fill-NoData -9999) are rendered with the nodata color
    let rgba = lut.colorize( &[4401.0, -9999.0, 32767.0], &[-9999.0, 32767.0], [255,0,255,128]);
    assert_eq!( rgba, vec![255,205,112,255, 255,0,255,128, 255,0,255,128]);

    let json: serde_json::Value = serde_json::from_str( &json_legend( &entries)).unwrap();
    let items = json.as_array().unwrap();
    assert_eq!( items.len(), entries.len() - 1); // no nodata entry
    assert_eq!( items[0]["code"], "4401");
    assert_eq!( items[0]["color"], "#ffcd70");
}

#[test]
fn test_json_color_map () {
    // manually edited ODIN JSON color maps (same format as legends) with unicode descriptions
    let json = r##"[
        { "color": "#686868", "code": "91", "descr": "NB1﹕urban/developed" },
        { "color": "#e6e640", "code": "106", "descr": "GR6﹕grass 1-2' ▒ ⛆" },
        { "color": "#ffffff", "code": "-9999", "descr": "Fill-NoData" }
    ]"##;
    let path = std::path::PathBuf::from( env!("CARGO_TARGET_TMPDIR")).join( "test_cmap.json");
    std::fs::write( &path, json).unwrap();

    let entries = read_lf_cmap( &path).unwrap(); // selected by file extension
    assert_eq!( entries.len(), 3);
    assert_eq!( entries[1].value, 106);
    assert_eq!( entries[1].name, "GR6﹕grass 1-2' ▒ ⛆");
    assert_eq!( entries[1].rgb, [0xe6, 0xe6, 0x40]);
    assert!( !entries[0].is_nodata);
    assert!( entries[2].is_nodata);

    let legend: serde_json::Value = serde_json::from_str( &json_legend( &entries)).unwrap();
    assert_eq!( legend.as_array().unwrap().len(), 2); // no nodata entry
    assert_eq!( legend[0]["descr"], "NB1﹕urban/developed");

    // CSV color maps are still read as before
    assert!( read_lf_cmap( std::path::Path::new("resources/LF2025_CBD.csv")).is_ok());
}
