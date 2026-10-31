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
//! directly as (odin_gdal::cmap) color maps for colorization of WMS GetMap images and color map legends

use std::{fs::File, io::Read, path::Path};
use csv::{ReaderBuilder, StringRecord};
use odin_gdal::{cmap::CmapEntry, errors::{Result as GdalResult, misc_error}};
use crate::errors::{OdinLandfireError, Result, op_failed};

/// name of landfire.gov CSV color map entries that represent nodata values
pub const FILL_NODATA: &str = "Fill-NoData";

/// the odin_gdal::cmap::CmapReader for landfire.gov CSV color maps (used by odin_gdal::wms_srv::WmsService)
pub fn read_lf_cmap (path: &Path)->GdalResult<Vec<CmapEntry>> {
    read_csv_color_map( path).map_err( |e| misc_error( e.to_string()))
}

/// read a landfire.gov CSV color map (including "Fill-NoData" entries, which are flagged as nodata)
pub fn read_csv_color_map<P: AsRef<Path>> (path: P)->Result<Vec<CmapEntry>> {
    let path = path.as_ref();
    let mut s = String::new();
    File::open( path)?.read_to_string( &mut s)?;
    let s = s.strip_prefix('\u{feff}').unwrap_or( &s); // some landfire.gov CSV files have a UTF-8 BOM

    let mut rdr = ReaderBuilder::new().has_headers(false).from_reader( s.as_bytes());
    let mut records = rdr.records();

    let hdrs = records.next().ok_or( op_failed!("empty CSV color map {:?}", path))?.map_err( csv_error)?;
    let fidx = FieldIndices::new( path, &hdrs)?;

    let mut entries: Vec<CmapEntry> = Vec::new();
    for rec in records {
        let rec = rec.map_err( csv_error)?;
        if let Some(value) = rec.get(fidx.value).and_then( |v| v.trim().parse::<i32>().ok())
        && let Some(name) = rec.get(fidx.name)
        && let Some(r) = rec.get(fidx.r).and_then( |v| v.trim().parse::<u8>().ok())
        && let Some(g) = rec.get(fidx.g).and_then( |v| v.trim().parse::<u8>().ok())
        && let Some(b) = rec.get(fidx.b).and_then( |v| v.trim().parse::<u8>().ok()) {
            entries.push( CmapEntry{ value, name: name.to_string(), rgb: [r,g,b], is_nodata: name == FILL_NODATA });
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
