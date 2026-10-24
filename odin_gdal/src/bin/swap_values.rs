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

//! in-place replacement of a pixel value in a raster band, e.g. to fix datasets that use de-facto nodata values which
//! differ from their declared nodata metadata. This is much faster than gdal_calc for large (tiled) datasets since it does
//! not create a copy. See odin_gdal::swap_rasterband_value() for details

use std::{path::Path, time::Instant};
use anyhow::{anyhow,Result};
use odin_common::define_cli;
use odin_gdal::{open_dataset_for_update, swap_rasterband_value};

define_cli! { ARGS [about="replace pixel values of a raster band in-place"] =
    band: usize [help="1-based band number", long, default_value="1"],
    src: Option<f64> [help="pixel value to replace", long, allow_hyphen_values=true],
    src_nodata: bool [help="replace pixels with the declared nodata value of the band", long],
    tgt: Option<f64> [help="value to replace with", long, allow_hyphen_values=true],
    tgt_nodata: bool [help="replace with the declared nodata value of the band", long],
    dry_run: bool [help="only count matching pixels, do not modify file", long],
    file: String [help="filename of dataset to modify"]
}

fn main()->Result<()> {
    let mut ds = open_dataset_for_update( Path::new( ARGS.file.as_str()))?;
    let band = ds.rasterband( ARGS.band)?;
    let nodata = band.no_data_value();

    let src = get_value( ARGS.src, ARGS.src_nodata, nodata, "src")?;
    let tgt = get_value( ARGS.tgt, ARGS.tgt_nodata, nodata, "tgt")?;

    let (cols,rows) = band.size();
    println!("{}: band {} of type {}, size {}x{}, block size {:?}, declared nodata: {:?}",
             ARGS.file, ARGS.band, band.band_type().name(), cols, rows, band.block_size(), nodata);
    println!("{} pixel value {src} with {tgt}", if ARGS.dry_run {"counting (dry run)"} else {"replacing"});
    drop(band);

    let t0 = Instant::now();
    let n = swap_rasterband_value( &mut ds, ARGS.band, src, tgt, ARGS.dry_run)?;

    println!("{} {} pixels in {:.1} sec", if ARGS.dry_run {"found"} else {"replaced"}, n, t0.elapsed().as_secs_f64());
    Ok(())
}

fn get_value (value: Option<f64>, use_nodata: bool, nodata: Option<f64>, name: &str)->Result<f64> {
    match (value, use_nodata) {
        (Some(v), false) => Ok(v),
        (None, true) => nodata.ok_or( anyhow!("band has no declared nodata value for --{name}-nodata")),
        (Some(_), true) => Err( anyhow!("--{name} and --{name}-nodata are mutually exclusive")),
        (None, false) => Err( anyhow!("either --{name} or --{name}-nodata is required"))
    }
}
