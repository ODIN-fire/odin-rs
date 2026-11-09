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

//! get_terrain_hit - compute the terrain intersection of a ray (e.g. a camera view direction) from a DEM, and report
//! the runtime. By default this uses `DemSource::ray_intersections` (native DEM resolution, lazily loaded block cache
//! for local DEMs, `RayIntersections` query for `serve_dem` servers). With `--grid` it uses an in-memory UTM `DemGrid`

use std::{path::PathBuf, time::Instant};
use anyhow::{anyhow, Result};
use odin_build::data_dir;
use odin_common::{define_cli, fs::EnvPathBuf, geo::GeoPoint};
use odin_dem::{DemGrid, DemSource, RayHit, RayQuery, ResampleAlg, DEFAULT_CURV_COEFF};

define_cli! { ARGS [about="get_terrain_hit - compute terrain intersection of a ray from a DEM"] =
    lon: f64 [help="ray origin longitude [deg]", long, allow_hyphen_values=true],
    lat: f64 [help="ray origin latitude [deg]", long, allow_hyphen_values=true],
    height: f64 [help="ray origin height above ground [m]", long, default_value="10"],
    bearing: f64 [help="ray bearing [deg] clockwise from true north", long],
    pitch: f64 [help="ray pitch [deg] (negative is down)", long, allow_hyphen_values=true],
    pitch_uncertainty: f64 [help="also compute near/far intersections for pitch +- this value [deg]", long, default_value="0"],
    max_dist: f64 [help="max horizontal ray distance [m]", long, default_value="40000"],
    curv_coeff: Option<f64> [help="earth curvature coefficient (1 - refraction, default 0.85714)", long],
    repeat: usize [help="number of ray casts to average runtime over", long, default_value="1"],
    grid: bool [help="use an in-memory UTM DemGrid around the ray origin instead of the native DEM", long],
    res: f64 [help="DemGrid resolution [m] (only with --grid)", long, default_value="30"],
    resample: String [help="DemGrid resampling algorithm (GDAL name: near, bilinear, cubic..) (only with --grid)", long, default_value="bilinear"],
    dem: Option<String> [help="odin_dem server URL or DEM VRT file (default: $ODIN_ROOT/data/odin_dem/3dep13-conus-i16/3dep13-conus-i16.vrt)", long]
}

#[tokio::main]
async fn main ()->Result<()> {
    odin_build::set_bin_context!();

    let src = match &ARGS.dem {
        Some(s) if s.starts_with("http://") || s.starts_with("https://") => DemSource::Server( s.clone()),
        Some(s) => DemSource::File( EnvPathBuf( PathBuf::from(s))),
        None => DemSource::File( EnvPathBuf( data_dir().join("odin_dem/3dep13-conus-i16/3dep13-conus-i16.vrt")))
    };
    let curv_coeff = ARGS.curv_coeff.unwrap_or( DEFAULT_CURV_COEFF);
    let mut rays = vec![ ("hit", ARGS.pitch) ];
    if ARGS.pitch_uncertainty > 0.0 {
        rays.push( ("near", ARGS.pitch - ARGS.pitch_uncertainty));
        rays.push( ("far", ARGS.pitch + ARGS.pitch_uncertainty));
    }
    let queries: Vec<RayQuery> = rays.iter().map( |(_,pitch)| {
        RayQuery { lon: ARGS.lon, lat: ARGS.lat, height: ARGS.height, above_ground: true, bearing: ARGS.bearing, pitch: *pitch,
                   max_dist: ARGS.max_dist, curv_coeff }
    }).collect();

    let n = ARGS.repeat.max(1);
    let (hits, t_first, t_rep) = if ARGS.grid {
        let alg = ResampleAlg::from_name( &ARGS.resample).ok_or( anyhow!("unknown resampling algorithm '{}'", ARGS.resample))?;
        let t0 = Instant::now();
        let grid = DemGrid::around( &src, &GeoPoint::from_lon_lat_degrees( ARGS.lon, ARGS.lat), ARGS.max_dist, ARGS.res, alg).await?;
        let (w,h) = grid.dimensions();
        println!("DEM grid {w}x{h} ({:.1} MB) retrieved in {:.3}s", (w*h*4) as f64 / 1e6, t0.elapsed().as_secs_f64());
        let hits = queries.iter().map( |q| grid.query(q)).collect::<Result<Vec<Option<RayHit>>,_>>()?;
        let t_first = t0.elapsed();

        let t1 = Instant::now();
        for _ in 0..n { for q in &queries { grid.query(q)?; } }
        (hits, t_first, t1.elapsed())

    } else {
        let t0 = Instant::now();
        let hits = src.ray_intersections( &queries).await?;
        let t_first = t0.elapsed();

        let t1 = Instant::now();
        for _ in 0..n { src.ray_intersections( &queries).await?; }
        (hits, t_first, t1.elapsed())
    };

    println!("first query of {} rays (including DEM access): {:.1}ms", queries.len(), t_first.as_secs_f64() * 1e3);
    println!("repeated queries: {:.1}µs per ray", t_rep.as_secs_f64() * 1e6 / (n * queries.len()) as f64);

    for ((label,pitch), hit) in rays.iter().zip( hits.iter()) {
        match hit {
            Some(hit) => println!("{label:>4} (pitch {pitch:.2}): {:.5},{:.5} height {:.1}m at {:.1}m", hit.lon, hit.lat, hit.height, hit.distance),
            None => println!("{label:>4} (pitch {pitch:.2}): no terrain intersection within {}m", ARGS.max_dist)
        }
    }

    Ok(())
}
