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

use std::path::Path;
use gdal::{Dataset, GdalOpenFlags};
use odin_common::define_cli;
use odin_gdal::{fill_nodata,FillNoDataAlg,open_dataset_for_update};
use odin_gdal::errors::Result;

define_cli! { ARGS [about="set NO_DATA value of raster data set"] =
    file: String [help="filename of dataset to modify"],
    value: i64 [help="NO_DATA integer value to set for all bands", allow_hyphen_values = true]
}

fn main()->Result<()> {
    let path = Path::new(ARGS.file.as_str());

    let mut ds = open_dataset_for_update( path)?;
    let n_bands = ds.raster_count();
    let value = ARGS.value;

    for i in 0..n_bands {
        let mut band = ds.rasterband(i + 1)?;
        band.set_no_data_value_i64( Some(value))?;
    }

    ds.flush_cache()?;

    Ok(())
}
