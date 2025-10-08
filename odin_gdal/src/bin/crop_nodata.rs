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

use std::path::Path;
use gdal::Dataset;
use odin_common::define_cli;
use odin_gdal::{crop_no_data, get_data_bounds, to_csl_string_list};
use odin_gdal::errors::Result;

define_cli! { ARGS [about="crop provided GDAL raster file so that it does not contain NO_DATA values"] =
    co: Vec<String> [help="create options", long],
    src_path: String [help="input filename"],
    tgt_path: Option<String> [help="(optional) output filename (only report crop boundaries if not present"]
}

fn main()->Result<()> {
    let src_path = Path::new(ARGS.src_path.as_str());
    let src_ds = Dataset::open(src_path)?;
    let (width,height) = src_ds.raster_size();

    println!("original size: width = {width}, height = {height}");
    let bbox = get_data_bounds( &src_ds, 1)?;
    println!("cropped size:  width = {}, height = {}", bbox.east - bbox.west, bbox.south - bbox.north);
    println!("cropped to {bbox:?}");

    if let Some(tgt_pathname) = &ARGS.tgt_path {
        let tgt_path = Path::new( tgt_pathname);
        let create_opts = to_csl_string_list(&ARGS.co)?;
        crop_no_data( &src_ds, tgt_path, create_opts)?;
    }
    Ok(())
}