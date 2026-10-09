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

use std::{path::Path,ops::Range};
use gdal::{Dataset, GdalOpenFlags};
use anyhow::{Result,anyhow};
use odin_common::define_cli;
use odin_gdal::{fill_nodata,FillNoDataAlg,open_dataset_for_update, colorize};

define_cli! { ARGS [about="colorize a tif data set to a PNG"] =
    min: f64 [help="min percentile to scale value range to", long, default_value="0.0"],
    max: f64 [help="max percentile to scale value range to", long, default_value="1.0"],
    band: usize [help="input band to colorize", long, default_value="1"],
    input: String [help="filename of dataset to colorize"],
    output: String [help="filename of png to generate"],
    rgba_list: String [help="comma separated list of rgba colors the range is mapped into linearly"]
}

fn main()->Result<()> {
    let src_path = Path::new(&ARGS.input);
    let dst_path = Path::new(&ARGS.output);

    let pct_range = ARGS.min..ARGS.max;
    let mut rgba_clrs: Vec<u32> = Vec::new();
    for sv in ARGS.rgba_list.as_str().split(','){
        rgba_clrs.push( u32::from_str_radix(sv.trim(),16)?);
    }

    let src_ds = Dataset::open(src_path)?;
    colorize::to_cmap( &dst_path, &src_ds, ARGS.band, Some(pct_range), &rgba_clrs)?;

    Ok(())
}
