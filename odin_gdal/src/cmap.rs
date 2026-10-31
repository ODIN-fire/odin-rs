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

//! pixel buffer colorization with color maps (for categorical integer data) and alpha encoding (for continuous data).
//! Color map entries are data source specific - reading them is up to the respective data source crates.
//! Note this is the buffer level counterpart of the `colorize` module, which colorizes whole datasets

use std::path::Path;
use crate::errors::Result;

/// a color map entry for a single (integer) data value
#[derive(Debug,Clone)]
pub struct CmapEntry {
    pub value: i32,
    pub name: String,
    pub rgb: [u8;3],
    pub is_nodata: bool, // value represents nodata (rendered with the requested nodata color)
}

impl CmapEntry {
    pub fn css_color (&self)->String {
        format!("#{:02x}{:02x}{:02x}", self.rgb[0], self.rgb[1], self.rgb[2])
    }
}

/// signature of data source specific color map readers
pub type CmapReader = dyn Fn(&Path)->Result<Vec<CmapEntry>> + Send + Sync;

/// JSON color map legend as used by odin_cesium/img_window.js: [{"color":"#rrggbb","code":"..","descr":".."},..]
/// note this does not include nodata entries
pub fn json_legend (entries: &[CmapEntry])->String {
    let items: Vec<serde_json::Value> = entries.iter().filter( |e| !e.is_nodata).map( |e| serde_json::json!({
        "color": e.css_color(),
        "code": e.value.to_string(),
        "descr": e.name
    })).collect();
    serde_json::to_string( &items).unwrap_or_else( |_| "[]".to_string())
}

/// is v a nodata value. NaN values are always nodata (float data sets)
#[inline]
pub fn is_nodata (v: f64, nodata: &[f64])->bool {
    v.is_nan() || nodata.contains( &v)
}

/// number of entries of value lookup tables (supports 8 and 16 bit integer data)
pub const LUT_SIZE: usize = 1 << 16;

/// a lookup table that maps all values of 8 or 16 bit integer data sets to RGBA colors. Values that are not in the
/// color map (or are nodata) are fully transparent. The base is the smallest value of the respective data type
/// (e.g. -32768 for Int16, 0 for UInt16 and UInt8)
pub struct ValueLut {
    rgba: Vec<[u8;4]>,
    base: i32,
}

impl ValueLut {
    pub fn new (entries: &[CmapEntry], base: i32)->Self {
        let mut lut = ValueLut { rgba: vec![ [0u8;4]; LUT_SIZE], base };
        for e in entries {
            if !e.is_nodata && let Some(idx) = lut.index( e.value) {
                lut.rgba[idx] = [e.rgb[0], e.rgb[1], e.rgb[2], 255];
            }
        }
        lut
    }

    #[inline]
    fn index (&self, v: i32)->Option<usize> {
        let i = v - self.base;
        if i >= 0 && (i as usize) < LUT_SIZE { Some(i as usize) } else { None }
    }

    #[inline]
    pub fn rgba (&self, v: i32)->[u8;4] {
        self.index(v).map( |i| self.rgba[i]).unwrap_or( [0,0,0,0])
    }

    /// colorize values. Nodata values are rendered with the nodata_rgba color
    pub fn colorize (&self, vs: &[f64], nodata: &[f64], nodata_rgba: [u8;4])->Vec<u8> {
        let mut buf: Vec<u8> = Vec::with_capacity( vs.len() * 4);
        for v in vs {
            if is_nodata( *v, nodata) {
                buf.extend_from_slice( &nodata_rgba);
            } else {
                buf.extend_from_slice( &self.rgba( *v as i32));
            }
        }
        buf
    }
}

/// colorize continuous values with a single RGB color, using the alpha channel to represent the value: values <= min
/// are fully transparent, values >= max are opaque. Nodata values are rendered with the nodata_rgba color
pub fn colorize_alpha (vs: &[f64], nodata: &[f64], nodata_rgba: [u8;4], rgb: [u8;3], min: f64, max: f64)->Vec<u8> {
    let mut buf: Vec<u8> = Vec::with_capacity( vs.len() * 4);
    let d = max - min;

    for v in vs {
        if is_nodata( *v, nodata) {
            buf.extend_from_slice( &nodata_rgba);
        } else {
            let a = if d <= 0.0 { 0 } else { (((*v - min) / d).clamp( 0.0, 1.0) * 255.0).round() as u8 };
            buf.extend_from_slice( &[rgb[0], rgb[1], rgb[2], a]);
        }
    }
    buf
}

/// parse a RRGGBB hex color spec (with optional leading '#')
pub fn parse_rgb (s: &str)->Option<[u8;3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() == 6 && let Ok(v) = u32::from_str_radix( s, 16) {
        Some( [ (v >> 16) as u8, (v >> 8) as u8, v as u8 ] )
    } else {
        None
    }
}

/// parse a RRGGBBAA or RRGGBB (opaque) hex color spec (with optional leading '#')
pub fn parse_rgba (s: &str)->Option<[u8;4]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() == 8 && let Ok(v) = u32::from_str_radix( s, 16) {
        Some( [ (v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8 ] )
    } else {
        parse_rgb( s).map( |rgb| [rgb[0], rgb[1], rgb[2], 255])
    }
}
