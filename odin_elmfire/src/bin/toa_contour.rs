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

use std::{path::{Path,PathBuf}};
use anyhow::{anyhow,Result};

use odin_gdal::gdal::Dataset;
use odin_common::define_cli;
use odin_elmfire::{contour_levels,pre_process_small_toa_raster};

define_cli! { ARGS [about="create contour vector dataset from small (pixelized) raster"] =
    n_hours: u32 [help="number of hours for isochrones"],
    src_filename: String [help="input filename"],
    tgt_filename: String [help="output filename"]
}

fn main () -> Result<()> {
    let src_path = Path::new(ARGS.src_filename.as_str());
    let tgt_path = Path::new(ARGS.tgt_filename.as_str());
    let src_ds = Dataset::open(src_path)?;
    let levels = contour_levels( ARGS.n_hours);

    let raw_json_path = "toa-raw.geojson";
    odin_gdal::contour::to_fixed_level_geo_json( &src_ds, 1, &levels, &raw_json_path)?;

    let mod_src_path = "toa_mod.tif";
    println!("creating shrink-wrapped and re-gridded source {:?}", mod_src_path);
    let mod_ds = pre_process_small_toa_raster( &src_ds, ARGS.n_hours, &mod_src_path)?;

    let json_path = "toa.geojson";
    println!("creating fixed level contour plot in {:?}", json_path);
    odin_gdal::contour::to_fixed_level_geo_json( &mod_ds, 1, &levels, &json_path)?;

    Ok(())
}
