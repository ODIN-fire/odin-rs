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

use std::{path::Path,fs::File,io::Write};
use gdal::Dataset;
use clap;
use anyhow::{Result, anyhow};
use odin_common::define_cli;
use odin_gdal::{RasterAttributeTable};

/// this tool is used to create ODIN or gdal color maps from partial landfire.gov
/// downloads which contain color maps encoded as raster attribute tables (RAT) inside
/// of respective *.tif files

define_cli! { ARGS [about="create color table from landfire.gov EVT dataset"] =
    format: String   [help="color table format (odin,gdal)", long, short, default_value="gdal"],
    src_path: String [help="dataset input filename"],
    tgt_path: String [help="colormap output filename"]
}

fn main()->Result<()> {
    let src_path = Path::new(ARGS.src_path.as_str());
    let ds = Dataset::open(src_path)?;

    if let Some(rat) = RasterAttributeTable::from_dataset( &ds, 1) {
        if ARGS.format == "odin" {
            let mut file = File::create( &ARGS.tgt_path)?;
            writeln!( file, "[");

            for row in 0..rat.n_rows {
                if let Some(v) = rat.get_i64_rat_field( row, 0)
                && let Some(name) = rat.get_string_rat_field( row, 2)
                && let Some(r) = rat.get_i64_rat_field( row, 14)
                && let Some(g) = rat.get_i64_rat_field( row, 15)
                && let Some(b) = rat.get_i64_rat_field( row, 16) {
                    let clr = format!("{:02x}{:02x}{:02x}", r, g, b);
                    //let a = if &name == "Fill-NoData" { 0 } else { 255 };
                    if row > 0 {
                        write!(file, ",\n{{ \"color\":\"{}\", \"code\":\"{}\", \"descr\":\"{}\" }}", clr, v, name);
                    } else {
                        write!(file, "{{ \"color\":\"{}\", \"code\":\"{}\", \"descr\":\"{}\" }}", clr, v, name);
                    }
                }
            }
            writeln!( file, "\n]");
            file.flush();
            Ok(())

        } else if ARGS.format == "gdal" {
            let mut file = File::create( &ARGS.tgt_path)?;

            for row in 0..rat.n_rows {
                if let Some(v) = rat.get_i64_rat_field( row, 0)
                && let Some(name) = rat.get_string_rat_field( row, 2)
                && let Some(r) = rat.get_i64_rat_field( row, 14)
                && let Some(g) = rat.get_i64_rat_field( row, 15)
                && let Some(b) = rat.get_i64_rat_field( row, 16) {
                    //let a = if &name == "Fill-NoData" { 0 } else { 255 };
                    writeln!(file, "{:5} {:3} {:3} {:3}", v, r, g, b );
                }
            }
            file.flush();
            Ok(())

        } else {
            Err( anyhow!("unknown colormap format {}", ARGS.format))
        }
    } else {
        Err( anyhow!("no raster attribute table in {}", ARGS.src_path))
    }
}
