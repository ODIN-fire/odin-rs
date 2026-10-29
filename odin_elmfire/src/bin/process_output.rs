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

use std::{path::{Path,PathBuf}, fs::remove_file};
use anyhow::{anyhow,Result};

use odin_gdal::{gdal::Dataset, get_exterior_data_bounds, warp::warp_to_wgs84_tif, crop_mem_dataset, open_dataset};
use odin_common::define_cli;
use odin_elmfire::{
    ElmfireRun, contour_levels, load_config, process_toa_contours, process_ef_raster_output, compute_timesteps_from_toa,
    wind::{create_computed_windfields, create_wx_windfields}
};
use uom::si::{f64::Area, area::square_meter};

define_cli! { ARGS [about="process Elmfire raster output from given output dir"] =
    config: Option<String> [help="name of Elmfire config file to use",long],
    t_start: i64 [help="seconds between base date and first ignition", long, default_value="0"],
    wind: bool [help="also create wind field outputs (requires elmfire.ron in parent of output_dir)", long],
    n_hours: u32 [help="number of hours for isochrones"],
    output_dir: String [help="output directory to process"]
}

fn main () -> Result<()> {
    let config = if let Some(cfg_path) = &ARGS.config { load_config(&cfg_path)? } else { load_config("elmfire.ron")? };
    let dir = Path::new( &ARGS.output_dir).to_path_buf();

    let raster_products = process_ef_raster_output( &config, &dir)?;
    for (fname,geo_rect) in raster_products.iter() {
        println!("generated {} with bbox {:?}", fname, geo_rect);
    }

    let line_path = dir.join("toa.geojson");
    let poly_path = dir.join("toa_poly.geojson");
    let ds = open_dataset( &dir.join("toa_wgs84.tif"))?;
    process_toa_contours( &ds, &line_path, &poly_path, config.small_fire_cells, ARGS.t_start, ARGS.n_hours)?;

    println!("generated contour files {:?} and {:?}", line_path, poly_path);

    if ARGS.wind {
        let run_dir = dir.parent().ok_or( anyhow!("output_dir has no parent"))?;
        let run = ElmfireRun::read_from( run_dir.join("elmfire.ron"))?;
        let ef = &run.fire;

        let cell_size = Area::new::<square_meter>( (config.lf_res_m as f64).powi(2));
        let timesteps = compute_timesteps_from_toa( &dir.join("toa.tif"), &ef.start_date, &contour_levels( ef.t_start(), ef.n_hours), cell_size)?;

        let windfields = if ef.compute_wind {
            create_computed_windfields( run_dir, &ef.base_date, &timesteps, config.wind_res_m)?
        } else {
            create_wx_windfields( run_dir, &ef.base_date, &timesteps, config.wind_res_m)?
        };

        println!("generated {} {} wind fields:", windfields.len(), if ef.compute_wind {"WindNinja"} else {"wx"});
        for wf in &windfields {
            println!("  {}: {}, {}, {}", wf.date, wf.grid, wf.vectors, wf.contours);
        }
    }

    Ok(())
}
