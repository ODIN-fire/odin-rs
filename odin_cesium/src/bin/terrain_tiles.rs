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

//! terrain_tiles - show local terrain configuration/availability and optionally pre-compute (seed) the tile cache for
//! low levels, which are the most expensive ones to compute since they cover many DEM source files.
//! Uses `$ODIN_ROOT/configs/odin_cesium/terrain.ron`

use std::time::Instant;
use anyhow::{anyhow, Result};
use odin_common::define_cli;
use odin_cesium::{load_config, terrain::{TerrainConfig, TerrainTiles}};

define_cli! { ARGS [about="terrain_tiles - show local terrain info and seed the tile cache"] =
    seed: Option<u32> [help="pre-compute all missing tiles up to (including) this level", long],
    jobs: Option<usize> [help="number of threads for seeding (default: number of CPUs)", short, long],
    tile: Option<String> [help="compute comma separated list of z/x/y tiles (without caching) and print their stats", long]
}

fn main ()->Result<()> {
    odin_build::set_bin_context!();

    let config: TerrainConfig = load_config("terrain.ron").map_err( |e| anyhow!("no terrain.ron config: {e}"))?;
    let t0 = Instant::now();
    let tiles = TerrainTiles::new( &config)?;
    println!("DEM:      {:?}", tiles.vrt_path);
    println!("geoid:    {:?}", tiles.geoid_path);
    println!("bounds:   [{:.4}, {:.4}, {:.4}, {:.4}]", tiles.bounds.0, tiles.bounds.1, tiles.bounds.2, tiles.bounds.3);
    println!("version:  {}", tiles.version);
    println!("cache:    {:?}", tiles.cache_dir);
    println!("layer.json: {} bytes ({:.1}ms)", tiles.layer_json().len(), t0.elapsed().as_secs_f64() * 1000.0);
    for (z, ranges) in tiles.availability.levels.iter().enumerate() {
        let n: u64 = ranges.iter().map( |r| (r.x1 - r.x0 + 1) as u64 * (r.y1 - r.y0 + 1) as u64).sum();
        println!("  level {z:2}: {:4} ranges, {n:10} tiles", ranges.len());
    }

    if let Some(specs) = &ARGS.tile {
        for spec in specs.split(',') {
            let v: Vec<u32> = spec.split('/').map( |s| s.trim().parse()).collect::<std::result::Result<_,_>>()?;
            if v.len() != 3 { return Err( anyhow!("tile has to be z/x/y")) }
            let t0 = Instant::now();
            let raw = tiles.compute_tile( v[0], v[1], v[2])?;
            let hs: Vec<f64> = raw[..raw.len()-1].chunks(2).map( |b| u16::from_le_bytes([b[0],b[1]]) as f64 / 5.0 - 1000.0).collect();
            let (min, max) = hs.iter().fold( (f64::MAX, f64::MIN), |a, h| (a.0.min(*h), a.1.max(*h)));
            println!("tile {spec}: {:.1}ms, heights [{min:.1}, {max:.1}], child mask {:#06b}", t0.elapsed().as_secs_f64() * 1000.0, raw[raw.len()-1]);
        }
    }

    if let Some(max_level) = ARGS.seed {
        let n_threads = ARGS.jobs.unwrap_or_else( || std::thread::available_parallelism().map( |n| n.get()).unwrap_or(4)).max(1);
        println!("seeding missing tiles up to level {max_level} using {n_threads} threads..");
        let t0 = Instant::now();
        let (n, n_err) = tiles.seed( max_level, n_threads, |i,n| {
            if i % 1000 == 0 { println!("  {i}/{n} tiles ({:.0}s)", t0.elapsed().as_secs_f64()) }
        });
        println!("seeded {n} tiles in {:.1}s ({n_err} errors)", t0.elapsed().as_secs_f64());
    }
    Ok(())
}
