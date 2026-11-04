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

//! tests for reading camera values from AlertCalifornia image overlays

use odin_alertca::overlay::{ImageOverlay, parse_image_overlay, read_image_overlay};
use odin_image::ocr::{create_default_ocr_engine, default_ocr_model_dir};

#[test]
fn test_parse_overlay () {
    let expected = ImageOverlay { elev_ft: 2812.0, azimut: 305.12, tilt: -1.78, zoom: 1.0 };

    let ov = parse_image_overlay( "axis-blackmtscc elev:2812' X:-54.88 Y:-1.78 Z:1.0 .ALERT OPS.slyon.10-30T14:11 © ALERTCalifornia 2026/11/03 13:38:53.52").unwrap();
    assert!( (ov.azimut - expected.azimut).abs() < 1e-9);
    assert_eq!( (ov.elev_ft, ov.tilt, ov.zoom), (expected.elev_ft, expected.tilt, expected.zoom));

    // OCR occasionally misses spaces
    let ov = parse_image_overlay( "axis-blackmtscc elev:2812'X:-54.88Y:-1.78Z:1.0.ALERT").unwrap();
    assert!( (ov.azimut - expected.azimut).abs() < 1e-9);

    // positive pan, explicit plus sign
    let ov = parse_image_overlay( "x elev:2316' X:179.40 Y:+2.5 Z:12.0 .ALERT").unwrap();
    assert_eq!( (ov.azimut, ov.tilt, ov.zoom), (179.4, 2.5, 12.0));

    assert!( parse_image_overlay( "ALERTCalifornia 2026/11/03 13:38:53.52").is_none());
}

#[test]
fn test_ocr_overlay () {
    // needs the OCR models (which are not part of the repository)
    if !default_ocr_model_dir().is_dir() {
        eprintln!("skipping OCR test - no models in {:?}", default_ocr_model_dir());
        return;
    }

    let engine = create_default_ocr_engine().unwrap();
    let t0 = std::time::Instant::now();
    let ov = read_image_overlay( &engine, "resources/blackmtscc-overlay.png").unwrap(); // bottom line of a 1920x1080 image
    println!("{ov:?} in {:?}", t0.elapsed());

    assert_eq!( ov.elev_ft, 2812.0);
    assert!( (ov.azimut - 305.12).abs() < 1e-9);
    assert_eq!( ov.tilt, -1.78);
    assert_eq!( ov.zoom, 1.0);
}
