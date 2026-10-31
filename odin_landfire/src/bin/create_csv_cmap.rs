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

#![allow(unused)]

use std::{fs::File, io::Write, collections::BTreeSet};
use anyhow::Result;
use odin_common::define_cli;
use odin_gdal::cmap::json_legend;
use odin_landfire::lf_cmap::read_csv_color_map;

/// this tool is used to create ODIN or gdal color maps from full landfire.gov data sets distributed as
/// zip files that already contain CSV color maps. Note that odin_landfire::LandfireService uses the CSV files directly,
/// i.e. this is only required for external tools (such as "gdal raster color-map ..")

define_cli! { ARGS [about="create color table from landfire.gov CSV table"] =
    format: String   [help="color table format (odin,gdal)", long, short, default_value="gdal"],
    alpha: bool      [help="add alpha channel", long, short],
    nodata: Vec<i32> [help="set extra no_data values",long],
    nd_color: Option<String> [help="override nodata color (in RRGGBBAA hex format)", long],
    src_path: String [help="dataset input filename"],
    tgt_path: String [help="colormap output filename"]
}

fn main()->Result<()> {
    let is_gdal_cmap = ARGS.format.eq_ignore_ascii_case("gdal");
    let entries = read_csv_color_map( &ARGS.src_path)?;

    let mut no_data_values: BTreeSet<i32> = ARGS.nodata.iter().cloned().collect();
    let mut nodata_clr = if let Some(rgba) = ARGS.nd_color.as_ref().map( |v| u32::from_str_radix(v, 16)).transpose()? {
        ((rgba >> 24) & 0xff, (rgba >> 16) & 0xff, (rgba >> 8) & 0xff, rgba & 0xff)
    } else {
        ( 0, 0, 0, 0 )   // might get overwritten by Fill-NoData record
    };

    for e in entries.iter().filter( |e| e.is_nodata) {
        if ARGS.nd_color.is_none() {
            nodata_clr = ( e.rgb[0] as u32, e.rgb[1] as u32, e.rgb[2] as u32, if ARGS.alpha { 0 } else { 255 } );
        }
        no_data_values.insert( e.value);
    }

    let mut cmap_file = File::create( &ARGS.tgt_path)?;

    if is_gdal_cmap {
        for e in entries.iter().filter( |e| !e.is_nodata) {
            if ARGS.alpha {
                writeln!( cmap_file, "{}  {} {} {} 255", e.value, e.rgb[0], e.rgb[1], e.rgb[2])?;
            } else {
                writeln!( cmap_file, "{}  {} {} {}", e.value, e.rgb[0], e.rgb[1], e.rgb[2])?;
            }
        }
        for value in no_data_values {
            writeln!( cmap_file, "{}  {} {} {} {}", value, nodata_clr.0, nodata_clr.1, nodata_clr.2, nodata_clr.3)?;
        }

    } else { // ODIN JSON cmap (no alpha, no nodata entries)
        write!( cmap_file, "{}", json_legend( &entries))?;
    }

    Ok(())
}
