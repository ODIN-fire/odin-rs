/*
 * Copyright © 2025, United States Government, as represented by the Administrator of 
 * the National Aeronautics and Space Administration. All rights reserved.
 *
 * The “ODIN” software is licensed under the Apache License, Version 2.0 (the "License"); 
 * you may not use this file except in compliance with the License. You may obtain a copy 
 * of the License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software distributed under
 * the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
 * either express or implied. See the License for the specific language governing permissions
 * and limitations under the License.
 */
#![allow(unused)]

use odin_dem::get_dem_heights;
use odin_common:: {define_cli, fs::{self, ensure_writable_dir}};
use std::path::{Path,PathBuf};

define_cli! { ARGS [about="get_dem - retrieve DEM file from given GDAL VRT"] =
    lon:  f64 [help="longitude in degrees", allow_hyphen_values = true, long],
    lat: f64 [help="latitude in degrees", allow_hyphen_values = true, long],
    vrt_file: String [help="path to GDAL *.vrt file to create the DEM from"]
}

fn main() {
    odin_build::set_bin_context!();

    if fs::existing_non_empty_file_from_path(&ARGS.vrt_file).is_ok() {

        let locations: Vec<(f64,f64)> = vec![ (ARGS.lon, ARGS.lat) ];
        match get_dem_heights( &ARGS.vrt_file, None, &locations) {
            Ok(heights) => {
                println!("height at {},{} = {} m", ARGS.lon, ARGS.lat, heights[0]);
            }
            Err(e) => eprintln!("failed to retrieve heights: {e}")
        }

    } else { eprintln!("VRT file not found {}", ARGS.vrt_file) }
}