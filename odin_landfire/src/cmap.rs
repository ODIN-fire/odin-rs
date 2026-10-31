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

//! color maps for Landfire data sets. Categorical data sets (EVT, FBFM40, CC, ..) are distributed by landfire.gov
//! with CSV files that contain VALUE, R, G and B columns plus a data set specific name column. We use those CSV files
//! directly to create value lookup tables for colorization of WMS GetMap images and ODIN JSON color maps (legends)

use std::{fs::File, io::Read, path::Path};
use csv::{ReaderBuilder, StringRecord};
use crate::errors::{OdinLandfireError, Result, op_failed};

pub const FILL_NODATA: &str = "Fill-NoData";

/// a single entry of a landfire.gov CSV color map
#[derive(Debug,Clone)]
pub struct CsvColorEntry {
    pub value: i32,
    pub name: String,
    pub rgb: [u8;3],
}

impl CsvColorEntry {
    pub fn is_nodata (&self)->bool { self.name == FILL_NODATA }

    pub fn css_color (&self)->String {
        format!("#{:02x}{:02x}{:02x}", self.rgb[0], self.rgb[1], self.rgb[2])
    }
}

/// read a landfire.gov CSV color map (including "Fill-NoData" entries)
pub fn read_csv_color_map<P: AsRef<Path>> (path: P)->Result<Vec<CsvColorEntry>> {
    let path = path.as_ref();
    let mut s = String::new();
    File::open( path)?.read_to_string( &mut s)?;
    let s = s.strip_prefix('\u{feff}').unwrap_or( &s); // some landfire.gov CSV files have a UTF-8 BOM

    let mut rdr = ReaderBuilder::new().has_headers(false).from_reader( s.as_bytes());
    let mut records = rdr.records();

    let hdrs = records.next().ok_or( op_failed!("empty CSV color map {:?}", path))?.map_err( csv_error)?;
    let fidx = FieldIndices::new( path, &hdrs)?;

    let mut entries: Vec<CsvColorEntry> = Vec::new();
    for rec in records {
        let rec = rec.map_err( csv_error)?;
        if let Some(value) = rec.get(fidx.value).and_then( |v| v.trim().parse::<i32>().ok())
        && let Some(name) = rec.get(fidx.name)
        && let Some(r) = rec.get(fidx.r).and_then( |v| v.trim().parse::<u8>().ok())
        && let Some(g) = rec.get(fidx.g).and_then( |v| v.trim().parse::<u8>().ok())
        && let Some(b) = rec.get(fidx.b).and_then( |v| v.trim().parse::<u8>().ok()) {
            entries.push( CsvColorEntry{ value, name: name.to_string(), rgb: [r,g,b] });
        }
    }

    if entries.is_empty() { Err( op_failed!("no color entries in {:?}", path)) } else { Ok(entries) }
}

fn csv_error (e: csv::Error)->OdinLandfireError {
    op_failed!("CSV error: {}", e)
}

struct FieldIndices { value: usize, name: usize, r: usize, g: usize, b: usize }

impl FieldIndices {
    fn new (path: &Path, hdrs: &StringRecord)->Result<Self> {
        let idx = |name: &str| hdrs.iter().position( |h| h.trim() == name);
        let name_col = path.file_name().and_then( |f| f.to_str()).and_then( get_lf_name_column);

        if let Some(value) = idx("VALUE")
        && let Some(r) = idx("R")
        && let Some(g) = idx("G")
        && let Some(b) = idx("B") {
            // fall back to the first column after VALUE if we don't know the name column of this data set
            let name = name_col.and_then( |n| idx(n)).unwrap_or( value + 1);
            Ok( FieldIndices{ value, name, r, g, b } )
        } else {
            Err( op_failed!("missing VALUE,R,G,B columns in {:?}", path))
        }
    }
}

/// the name of the descriptive column for known landfire.gov CSV files
pub fn get_lf_name_column (fname: &str)->Option<&'static str> {
    let fname = fname.to_lowercase();
    if fname.ends_with("_evt.csv") { Some("EVT_NAME") }
    else if fname.ends_with("_fvt.csv") { Some("EVT_FUEL_N") }
    else if fname.ends_with("_fbfm40.csv") { Some("FBFM40") }
    else if fname.ends_with("_cc.csv") { Some("CC_PERCENT") }
    else if fname.ends_with("_ch.csv") { Some("METERS") }
    else if fname.ends_with("_cbd.csv") { Some("KGM3") }
    else if fname.ends_with("_cbh.csv") { Some("METERS") }
    //... and a lot more to follow
    else { None }
}

/// ODIN JSON color map (legend) as used by odin_cesium/img_window.js: [{"color":"#rrggbb","code":"..","descr":".."},..]
/// note this does not include nodata entries
pub fn odin_json_color_map (entries: &[CsvColorEntry])->String {
    let items: Vec<serde_json::Value> = entries.iter().filter( |e| !e.is_nodata()).map( |e| serde_json::json!({
        "color": e.css_color(),
        "code": e.value.to_string(),
        "descr": e.name
    })).collect();
    serde_json::to_string( &items).unwrap_or_else( |_| "[]".to_string())
}

/// a lookup table that maps all possible i16 values to RGBA colors. Values that are not in the color map (or are
/// "Fill-NoData") are fully transparent
pub struct ValueLut {
    rgba: Vec<[u8;4]>
}

impl ValueLut {
    pub fn from_entries (entries: &[CsvColorEntry])->Self {
        let mut rgba = vec![ [0u8;4]; 1 << 16];
        for e in entries {
            if !e.is_nodata() && e.value >= i16::MIN as i32 && e.value <= i16::MAX as i32 {
                rgba[ Self::index(e.value as i16)] = [e.rgb[0], e.rgb[1], e.rgb[2], 255];
            }
        }
        ValueLut { rgba }
    }

    #[inline]
    fn index (v: i16)->usize { (v as i32 - i16::MIN as i32) as usize }

    #[inline]
    pub fn rgba (&self, v: i16)->[u8;4] { self.rgba[ Self::index(v)] }

    /// colorize values. Nodata values are rendered with the nodata_rgba color
    pub fn colorize (&self, vs: &[i16], nodata: &[i16], nodata_rgba: [u8;4])->Vec<u8> {
        let mut buf: Vec<u8> = Vec::with_capacity( vs.len() * 4);
        for v in vs {
            if nodata.contains(v) {
                buf.extend_from_slice( &nodata_rgba);
            } else {
                buf.extend_from_slice( &self.rgba(*v));
            }
        }
        buf
    }
}

/// colorize continuous values with a single RGB color, using the alpha channel to represent the value: values <= min
/// are fully transparent, values >= max are opaque. Nodata values are rendered with the nodata_rgba color
pub fn colorize_alpha (vs: &[i16], nodata: &[i16], nodata_rgba: [u8;4], rgb: [u8;3], min: f64, max: f64)->Vec<u8> {
    let mut buf: Vec<u8> = Vec::with_capacity( vs.len() * 4);
    let d = max - min;

    for v in vs {
        if nodata.contains(v) {
            buf.extend_from_slice( &nodata_rgba);
        } else {
            let a = if d <= 0.0 { 0 } else { (((*v as f64 - min) / d).clamp( 0.0, 1.0) * 255.0).round() as u8 };
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
