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

//! encode_dem - convert DEM source tiles (e.g. original Float32 3DEP tiles) into tiled GeoTIFFs with overviews,
//! using one of the [`DemEncoding`]s (U16 (default), I16 or F32). The encoding is taken from the `encoding` field of
//! `$ODIN_ROOT/configs/odin_dem/dem.ron` unless it is set with `--encoding`.
//! Existing output files are skipped unless `--force` is set, i.e. interrupted runs can be continued.

use std::{path::PathBuf, sync::{atomic::{AtomicUsize, AtomicU64, Ordering}, Mutex}, time::Instant};
use anyhow::{anyhow, Result};
use odin_common::define_cli;
use odin_dem::{collect_dem_files, encode_dem_file, load_config, DemConfig, DemEncoding, EncodeOptions};

define_cli! { ARGS [about="encode_dem - convert DEM source tiles into encoded, tiled GeoTIFFs with overviews"] =
    encoding: Option<DemEncoding> [help="encoding: u16, i16 or f32 (default: dem.ron 'encoding', or u16)", long],
    output_dir: PathBuf [help="output directory for encoded tiles", short, long],
    jobs: Option<usize> [help="number of files to encode in parallel (default: number of CPUs)", short, long],
    force: bool [help="re-encode existing output files", long],
    ovr_levels: Option<String> [help="comma separated overview levels (default: 4,8,16,32,64; 'none' for no overviews)", long],
    compress: String [help="GTiff compression", long, default_value="ZSTD"],
    inputs: Vec<PathBuf> [help="source tiles or directories with source tiles (*.tif)", required=true]
}

fn main ()->Result<()> {
    odin_build::set_bin_context!();

    let encoding = match ARGS.encoding {
        Some(enc) => enc,
        None => load_config::<DemConfig>("dem.ron").map( |c| c.encoding).unwrap_or_default()
    };
    let mut opts = EncodeOptions { compress: ARGS.compress.clone(), ..EncodeOptions::default() };
    if let Some(levels) = &ARGS.ovr_levels {
        opts.ovr_levels = if levels == "none" { Vec::new() } else {
            levels.split(',').map( |l| l.trim().parse::<i32>()).collect::<std::result::Result<_,_>>().map_err( |e| anyhow!("invalid overview levels: {e}"))?
        };
    }

    let files = collect_dem_files( &ARGS.inputs)?;
    std::fs::create_dir_all( &ARGS.output_dir)?;
    let jobs: Vec<(PathBuf,PathBuf)> = files.into_iter()
        .map( |src| { let dst = ARGS.output_dir.join( src.file_name().unwrap()); (src, dst) })
        .filter( |(_,dst)| ARGS.force || !dst.is_file())
        .collect();
    let n_threads = ARGS.jobs.unwrap_or_else( || std::thread::available_parallelism().map( |n| n.get()).unwrap_or(4)).max(1).min( jobs.len().max(1));

    println!("encoding {} files as {encoding} into {:?} using {n_threads} threads (overviews {:?})", jobs.len(), ARGS.output_dir, opts.ovr_levels);
    let t0 = Instant::now();
    let next = AtomicUsize::new(0);
    let (src_bytes, dst_bytes, n_done) = (AtomicU64::new(0), AtomicU64::new(0), AtomicUsize::new(0));
    let errors: Mutex<Vec<String>> = Mutex::new( Vec::new());

    std::thread::scope( |s| {
        for _ in 0..n_threads {
            s.spawn( || loop {
                let i = next.fetch_add( 1, Ordering::Relaxed);
                if i >= jobs.len() { break }
                let (src, dst) = &jobs[i];
                match encode_dem_file( src, dst, encoding, &opts) {
                    Ok(st) => {
                        src_bytes.fetch_add( st.src_bytes, Ordering::Relaxed);
                        dst_bytes.fetch_add( st.dst_bytes, Ordering::Relaxed);
                        let k = n_done.fetch_add( 1, Ordering::Relaxed) + 1;
                        let clamped = if st.n_clamped > 0 { format!(", {} CLAMPED", st.n_clamped) } else { String::new() };
                        println!("[{k}/{}] {:?}: {:.1} -> {:.1} MB ({:.0}%), max err {:.3}m, {:.1}% nodata{clamped}, {:.1}s",
                                 jobs.len(), dst.file_name().unwrap(), st.src_bytes as f64 / 1e6, st.dst_bytes as f64 / 1e6,
                                 100.0 * st.dst_bytes as f64 / st.src_bytes.max(1) as f64, st.max_error,
                                 100.0 * st.n_nodata as f64 / st.n_pixels.max(1) as f64, st.secs);
                    }
                    Err(e) => {
                        eprintln!("ERROR encoding {src:?}: {e}");
                        errors.lock().unwrap().push( format!("{src:?}: {e}"));
                    }
                }
            });
        }
    });

    let (sb, db) = (src_bytes.load( Ordering::Relaxed), dst_bytes.load( Ordering::Relaxed));
    println!("encoded {} files in {:.1}s: {:.2} GB -> {:.2} GB ({:.0}%)", n_done.load( Ordering::Relaxed), t0.elapsed().as_secs_f64(),
             sb as f64 / 1e9, db as f64 / 1e9, 100.0 * db as f64 / sb.max(1) as f64);
    let errors = errors.into_inner().unwrap();
    if errors.is_empty() { Ok(()) } else { Err( anyhow!("{} files failed to encode", errors.len())) }
}
