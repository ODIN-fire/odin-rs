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

//! compute the overview mosaic of the cached product time steps (latest of each configured station) and show the
//! time spent in each step and the size of the result. This is used to tune mosaic parameters (see `mosaic` of the
//! NexradConfig), command line options override config values

use std::{path::PathBuf, sync::Arc, time::{Duration, Instant}};
use clap::Parser;
use anyhow::{Result, anyhow};
use odin_nexrad::{
    CACHE_DIR, NexradConfig, configured_stations, load_config,
    mosaic::{MosaicBuilder, MosaicConfig, StationGrid, blend, smooth, contour, mosaic_grid, mosaic_inputs},
    store::load_cached_store,
};

#[derive(Parser, Debug)]
#[command(version, about, long_about = "compute NEXRAD overview mosaics from cached product time steps")]
struct Args {
    /// NexradConfig file (stations, products and mosaic parameters)
    #[arg(long, default_value="norcal_nexrad.ron")]
    config: String,

    /// mosaic cell size (meters)
    #[arg(long)]
    resolution: Option<f64>,

    /// gaussian smoothing sigma (meters)
    #[arg(long)]
    smooth: Option<f64>,

    /// station transition zone (meters)
    #[arg(long)]
    blend: Option<f64>,

    /// polygon simplification tolerance (meters)
    #[arg(long)]
    simplify: Option<f64>,

    /// min polygon/hole area (km²)
    #[arg(long)]
    min_area: Option<f64>,

    /// comma separated contour levels (dBZ)
    #[arg(long)]
    levels: Option<String>,

    /// directory to write the GeoJSON files to (default is the mosaic dir in the cache)
    #[arg(long)]
    out: Option<PathBuf>,
}

fn main()->Result<()> {
    let args = Args::parse();
    let config: NexradConfig = load_config( &args.config)?;
    let mut mc = config.mosaic.clone().unwrap_or_else( || MosaicConfig {
        interval: Duration::from_secs(120), max_age: Duration::from_secs(900), max_mosaics: 12,
        resolution_m: 1000.0, smooth_m: 1500.0, blend_m: 20000.0, simplify_m: 250.0, min_area_km2: 4.0,
        levels: vec![10.0, 20.0, 30.0, 40.0, 50.0]
    });
    if let Some(v) = args.resolution { mc.resolution_m = v }
    if let Some(v) = args.smooth { mc.smooth_m = v }
    if let Some(v) = args.blend { mc.blend_m = v }
    if let Some(v) = args.simplify { mc.simplify_m = v }
    if let Some(v) = args.min_area { mc.min_area_km2 = v }
    if let Some(s) = &args.levels { mc.levels = s.split(',').map( |l| l.trim().parse::<f32>()).collect::<std::result::Result<_,_>>()? }
    println!("{mc:?}");

    let stations = configured_stations( &config);
    let store = load_cached_store( stations.clone(), &config.products, usize::MAX, &CACHE_DIR);
    let grid = mosaic_grid( &stations, mc.resolution_m)?;
    println!("mosaic grid: {}x{} cells", grid.width, grid.height);
    let no_echo = mc.no_echo_dbz();

    for product in &config.products {
        let latest = store.stations.iter().filter_map( |s| s.product( *product).and_then( |p| p.latest()));
        let entries = mosaic_inputs( latest, mc.max_age);
        if entries.is_empty() { println!("{}: no inputs", product.name()); continue }
        println!("\n{}: {}", product.name(), entries.iter().map( |e| format!("{} {}", e.station, e.scan_time.format("%H:%M:%S"))).collect::<Vec<_>>().join(", "));

        //--- individual steps (timing)
        let t = Instant::now();
        let mut grids = Vec::new();
        for e in &entries {
            let station = stations.iter().find( |s| s.id == e.station).ok_or( anyhow!("unknown station"))?;
            grids.push( Arc::new( StationGrid::from_entry( e, station, mc.resolution_m, no_echo)?));
        }
        let t_read = t.elapsed();

        let t = Instant::now();
        let mut values = blend( &grids, &grid, mc.blend_m, no_echo);
        let t_blend = t.elapsed();

        let t = Instant::now();
        smooth( &mut values, grid.width, grid.height, mc.smooth_m / mc.resolution_m);
        let t_smooth = t.elapsed();

        let echo = values.iter().filter( |v| **v >= mc.levels[0]).count();
        println!("  cells >= {} dBZ: {:.1}%", mc.levels[0], 100.0 * echo as f64 / values.len() as f64);

        let t = Instant::now();
        let raw = contour( &values, &grid, &mc.levels, 0.0, 0.0)?;
        let t_contour_raw = t.elapsed();
        let t = Instant::now();
        let bands = contour( &values, &grid, &mc.levels, mc.simplify_m, mc.min_area_km2)?;
        let t_contour = t.elapsed();

        println!("  read+reduce {:?}, blend {:?}, smooth {:?}, contour {:?} (without simplification {:?})", t_read, t_blend, t_smooth, t_contour, t_contour_raw);
        println!("  raw: {} polygons, {} vertices", raw.iter().map( |b| b.polygons.len()).sum::<usize>(), raw.iter().map( |b| b.n_vertices()).sum::<usize>());
        for b in &bands {
            println!("  band {:>5} - {:<5}: {:5} polygons {:7} vertices", b.lo, b.hi.map( |h| h.to_string()).unwrap_or("".into()), b.polygons.len(), b.n_vertices());
        }

        //--- complete build (as done by the actor)
        let out_dir = args.out.clone().unwrap_or( CACHE_DIR.clone());
        let mut builder = MosaicBuilder::new( mc.clone(), stations.clone(), config.min_dbz, &out_dir)?;
        let m = builder.build( *product, &entries)?;
        let size = std::fs::metadata( &m.path)?.len();
        println!("  build: {:?}, {} polygons, {} vertices, {} bytes -> {:?}", m.compute_time, m.n_polygons, m.n_vertices, size, m.path);
    }
    Ok(())
}
