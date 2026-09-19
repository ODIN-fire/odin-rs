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

use anyhow::{Result,anyhow};
use std::path::Path;
use gdal::{Dataset, GdalOpenFlags};
use odin_common::define_cli;
use odin_gdal::grid::resample_ds;
use odin_gdal::{FillNoDataAlg, fill_nodata, get_driver_from_filename, open_update, get_default_create_opts_from_filename};

define_cli! { ARGS [about="resample dataset by given scale factors"] =
    in_file: String [help="input filename"],
    out_file: String [help="output filename"],
    scale_x: f64 [help="horizontal scale factor"],
    scale_y: f64 [help="vertical scale factor"]
}

fn main()->Result<()> {
    let in_path = Path::new( ARGS.in_file.as_str());
    let out_path = Path::new( ARGS.out_file.as_str());

    let src_ds = Dataset::open( in_path)?;
    let drv = get_driver_from_filename(  &ARGS.out_file.as_str()).ok_or(anyhow!("unknown output file type"))?;
    let create_opts = get_default_create_opts_from_filename( &ARGS.out_file.as_str());

    let ds = resample_ds::<f32,_>( &drv, &out_path, &src_ds, ARGS.scale_x, ARGS.scale_y, Some(create_opts))?;
    println!("resampled to: {:?}", out_path);

    Ok(())
}
