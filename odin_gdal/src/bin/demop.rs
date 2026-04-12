/*
 * Copyright © 2026, United States Government, as represented by the Administrator of
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
use gdal::{Dataset};

use odin_common::define_cli;
use odin_gdal::dem::{GdalDemAlg, GdalDemOp, create_dem_ds};

define_cli! { ARGS [about="demop - create derived DEM dataset from elevation"] =
    rough_terrain: bool [help="terrain is rough (use Horn algorithm)", long,short],

    op: String [help="DEM operation [slope,aspect]"],
    in_path: String [help="path to input dataset (elevation)"],
    out_path: String [help="path to output dataset with aspect/slope"]
}

fn main()->Result<()> {
    let op = match ARGS.op.as_str() {
        "slope" => GdalDemOp::Slope,
        "aspect" => GdalDemOp::Aspect,
        _ => return Err( anyhow!("unsupported operation: {}", ARGS.op))
    };

    let alg = if ARGS.rough_terrain { GdalDemAlg::Horn } else { GdalDemAlg::ZevenbergenThorne };

    let elev_ds = Dataset::open(&ARGS.in_path)?;
    let dem_ds = create_dem_ds( &elev_ds, &ARGS.out_path, op, alg, true, true)?;

    println!("created {} dataset: {}", ARGS.op, ARGS.out_path);
    Ok(())
}
