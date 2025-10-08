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

use std::path::Path;
use gdal::{Dataset, GdalOpenFlags};
use odin_common::define_cli;
use odin_gdal::{fill_nodata,FillNoDataAlg,open_update};
use odin_gdal::errors::Result;

define_cli! { ARGS [about="fill GDAL raster file with NO_DATA values"] =
    path: String [help="filename of dataset to fill"],
    alg: FillNoDataAlg  [help="which extrapolation algorithm to use", long, value_enum, default_value="inverse-distance"],
    smooth: usize [help="number of smoothing iterations (default 3)", long, default_value="3"],
    max_dist: usize [help="maximum pixel distance to fill from", long, default_value="100"]
}

fn main()->Result<()> {
    let path = Path::new(ARGS.path.as_str());

    let mut ds = open_update( path)?;

    fill_nodata( &mut ds, ARGS.max_dist, ARGS.smooth, ARGS.alg)?;

    Ok(())
}