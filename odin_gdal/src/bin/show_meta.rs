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

use std::path::{Path,PathBuf};
use anyhow::{anyhow,Result};
use gdal::{Dataset,raster::RasterBand,Metadata};

use odin_common::define_cli;

define_cli! { ARGS [about="show_meta - show meta information for raster bands of GDAL dataset"] =
    path: String [help="path to GDAL dataset to analyze"]
    // possibly more in the future
}

fn main ()->Result<()> {
    let path = Path::new( &ARGS.path).to_path_buf();
    let ds = Dataset::open(&path)?;

    let (rows,cols) = ds.raster_size();
    println!("raster size: {},{}", rows,cols);
    show_meta( &ds, 0)?;

    for i in 0..ds.raster_count() {
        let band_id = i+1;
        println!("--- band {}", band_id);
        let band = ds.rasterband( band_id)?;
        show_meta( &band, 4)?;
    }

    Ok(())
}

fn show_meta<M> (meta: &M, level: usize)->Result<()> where M: Metadata {
    let indent = String::from_utf8(vec![b' '; level])?;

    if let Ok(descr) = meta.description() { 
        println!("{}description: {}", indent, descr);
    }

    for domain in meta.metadata_domains() {
        if let Some(items) = meta.metadata_domain( &domain) {
            if !items.is_empty() {
                println!("{}domain: {}", indent, domain);
                for item in &items {
                    println!("{}    {}", indent, item);
                }
            }
        }
    }

    Ok(())
}
