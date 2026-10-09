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

use std::{path::{Path,PathBuf}, fs::remove_file};
use anyhow::{anyhow,Result};

use odin_gdal::{gdal::Dataset, get_exterior_data_bounds, warp::warp_to_wgs84_tif, crop_mem_dataset, open_dataset};
use odin_common::define_cli;
use odin_elmfire::{contour_levels, load_config, process_contour, process_ef_raster_output};

define_cli! { ARGS [about="process Elmfire raster output from given output dir"] =
    config: Option<String> [help="name of Elmfire config file to use",long],
    t_start: i64 [help="seconds between base date and first ignition", long, default_value="0"],
    n_hours: u32 [help="number of hours for isochrones"],
    output_dir: String [help="output directory to process"]
}

fn main () -> Result<()> {
    let config = if let Some(cfg_path) = &ARGS.config { load_config(&cfg_path)? } else { load_config("elmfire.ron")? };
    let dir = Path::new( &ARGS.output_dir).to_path_buf();

    let raster_products = process_ef_raster_output( &config, &dir)?;
    for (fname,geo_rect) in raster_products.iter() {
        println!("generated {} with bbox {:?}", fname, geo_rect);
    }

    let path = dir.join("toa.geojson");
    let ds = open_dataset( &dir.join("toa_wgs84.tif"))?;
    process_contour( &ds, &path, config.small_fire_cells, ARGS.t_start, ARGS.n_hours)?;

    println!("generated contour file in {:?}", path);

    Ok(())
}
