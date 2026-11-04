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

//! extraction of camera state values from the text overlay of AlertCalifornia images. The overlay is a single line at
//! the bottom of each image, e.g. `axis-blackmtscc elev:2812' X:-54.88 Y:-1.78 Z:1.0 .ALERT OPS.. 2026/11/03 13:38:53.52`
//! with camera elevation [ft], pan (X, [-180,180] degrees), tilt (Y, degrees) and zoom factor (Z). These values are
//! exact for the respective image, whereas the AlertCalifornia camera data (`all_cameras`) is updated separately and
//! does not include PTZ values for all cameras

use std::path::Path;
use lazy_static::lazy_static;
use regex::Regex;
use odin_image::ocr::{OcrEngine, recognize_bottom_text_in_file};
use crate::errors::{OdinAlertCaError, Result, op_failed};

/// height of the image overlay text line (in pixels of 1920x1080 images)
pub const OVERLAY_HEIGHT: u32 = 30;

lazy_static! {
    // note that OCR occasionally misses spaces
    static ref OVERLAY_RE: Regex = Regex::new(
        r"elev:\s*(\d+(?:\.\d+)?)'?\s*X:\s*([-+]?\d+(?:\.\d+)?)\s*Y:\s*([-+]?\d+(?:\.\d+)?)\s*Z:\s*(\d+(?:\.\d+)?)"
    ).unwrap();
}

/// the camera state values of an image overlay
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageOverlay {
    pub elev_ft: f64,
    pub azimut: f64, // [0,360) degrees (converted from pan)
    pub tilt: f64,   // degrees (negative is downwards)
    pub zoom: f64,   // zoom factor
}

/// parse the overlay values from a (recognized) overlay text
pub fn parse_image_overlay (text: &str)->Option<ImageOverlay> {
    let caps = OVERLAY_RE.captures( text)?;
    let elev_ft: f64 = caps.get(1)?.as_str().parse().ok()?;
    let pan: f64 = caps.get(2)?.as_str().parse().ok()?;
    let tilt: f64 = caps.get(3)?.as_str().parse().ok()?;
    let zoom: f64 = caps.get(4)?.as_str().parse().ok()?;

    if pan.abs() > 360.0 || tilt.abs() > 90.0 { return None } // OCR artifact
    let azimut = if pan < 0.0 { pan + 360.0 } else { pan }; // avoid rounding errors for positive pan angles
    Some( ImageOverlay { elev_ft, azimut, tilt, zoom })
}

/// recognize and parse the overlay text of the image file at `path`. This is blocking (CPU intensive)
pub fn read_image_overlay (engine: &OcrEngine, path: impl AsRef<Path>)->Result<ImageOverlay> {
    let mut texts = recognize_bottom_text_in_file( engine, path.as_ref(), OVERLAY_HEIGHT)
        .map_err( |e| op_failed!("overlay OCR failed: {}", e))?;
    texts.sort_by_key( |t| t.left); // OCR might split the line into several text boxes
    let line = texts.iter().map( |t| t.text.as_str()).collect::<Vec<_>>().join(" ");

    parse_image_overlay( &line).ok_or_else( || op_failed!("no camera values in overlay text '{}'", line))
}
