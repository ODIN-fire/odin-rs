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

//! build_dem_vrt - create a GDAL VRT from DEM source tiles (as created by `encode_dem` or original 3DEP Float32 tiles).
//! The resulting VRT always returns meters (Int16 for I16, Float32 for U16 and F32), i.e. clients do not need to know
//! the encoding. U16 VRTs consist of three files (`<name>.vrt` referencing `<name>.u16x.vrt` and `<name>.u16.vrt`), see
//! `odin_dem::encoding::build_dem_vrt`. All sources have to use the same encoding.
//! Sources are added in the order of the arguments (files of directories sorted by name), later sources take precedence
//! where they overlap, which can be used to add higher resolution regions on top of coarser base layers.
//!
//! Encoding and output path default to the `encoding` and `vrt_path` fields of `$ODIN_ROOT/configs/odin_dem/dem.ron`.

use std::path::PathBuf;
use anyhow::{anyhow, Result};
use odin_common::define_cli;
use odin_dem::{build_dem_vrt, collect_dem_files, load_config, DemConfig, DemEncoding};

define_cli! { ARGS [about="build_dem_vrt - create a GDAL VRT from DEM source tiles"] =
    encoding: Option<DemEncoding> [help="source encoding: u16, i16 or f32 (default: dem.ron 'encoding', or u16)", long],
    output: Option<PathBuf> [help="VRT file to create (default: dem.ron 'vrt_path')", short, long],
    force: bool [help="overwrite an existing VRT file", long],
    inputs: Vec<PathBuf> [help="source tiles or directories with source tiles (*.tif)", required=true]
}

fn main ()->Result<()> {
    odin_build::set_bin_context!();

    let config: Option<DemConfig> = load_config("dem.ron").ok();
    let encoding = ARGS.encoding.or( config.as_ref().map( |c| c.encoding)).unwrap_or_default();
    let vrt_path = match (&ARGS.output, &config) {
        (Some(path), _) => path.clone(),
        (None, Some(c)) => c.vrt_path.0.clone(),
        (None, None) => return Err( anyhow!("no --output given and no dem.ron config found"))
    };
    if vrt_path.exists() && !ARGS.force {
        return Err( anyhow!("{vrt_path:?} already exists (use --force to overwrite)"))
    }

    let files = collect_dem_files( &ARGS.inputs)?;
    println!("building {encoding} VRT {vrt_path:?} from {} source files..", files.len());
    let stats = build_dem_vrt( &files, &vrt_path, encoding)?;

    for w in &stats.warnings { println!("WARNING: {w}"); }
    for f in &stats.files { println!("wrote {f:?}"); }
    println!("created {vrt_path:?}: {} sources, {}x{} pixels, resolution {:e} x {:e}, bounds [{:.4}, {:.4}, {:.4}, {:.4}]",
             stats.n_sources, stats.width, stats.height, stats.res.0, stats.res.1,
             stats.bounds[0], stats.bounds[1], stats.bounds[2], stats.bounds[3]);
    Ok(())
}
