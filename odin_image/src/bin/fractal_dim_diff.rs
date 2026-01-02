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

use odin_common::define_cli;
use odin_image::{
    blackout_sky, fractal_dim, fractal_dim_of_tile, get_dominant_tile_size, get_grid_dim, get_horizon, 
    open_diff_image_pair, process_subimage_tiles_mut, Mask, TileData
};
use anyhow::{Result};

define_cli! { ARGS [about="estimate fractal dimension of given image"] =
    top_margin: u32 [help="optional top margin to crop from input image", long, default_value="0"],
    bottom_margin: u32 [help="optional bottom margin to crop from input image", long, default_value="0"],

    //--- tiling parameters
    fractional_tiles: bool [help="process fractional tiles at right/bottom image", long],
    n: usize [help="number of tiles", long, short, default_value="10"],
    vertical: bool [help="tile vertically",long],
    keep_ratio: bool [help="use aspect-ratio tile sizes (default is square)",long],

    //--- horizon detection parameters
    y_dist: u32 [help="horizontal distance in pixels to determine V,S gradients", long, default_value="20"],
    v_diff: f32 [help="min V difference (gradient) we consider to be the horizon line [0..-1]", long, default_value="-0.1"],
    s_diff: f32 [help="min S difference (gradient) we consider to be the horizon line [0..1]", long, default_value="0.1"],
    loess_width: usize [help="bandwidth for LOESS smoothing of horizon edge", long, default_value="20"],
    offset: i32 [help="horizon offset in pixels", long, allow_hyphen_values = true, default_value="0"],

    mask: Option<String> [help="optional mask file to use", long],
    horizon: Option<String> [help="optional horizon file to use", long],

    min_scale: f32 [help="minimum scale factor [0..1]", long, default_value="0.4"],

    src_file1: String [help="filename of first image to compare"],
    src_file2: String [help="filename of second image to compare"]
}

fn main()->Result<()> {
    let (mut img1, mut img2) = open_diff_image_pair( &ARGS.src_file1, &ARGS.src_file2)?;
    let horizon = get_horizon( ARGS.horizon.as_ref(), &img1, ARGS.top_margin, ARGS.y_dist, ARGS.v_diff, ARGS.s_diff, ARGS.loess_width)?;

    // make sure sky doesn't contribute to fractal dimension difference
    blackout_sky(&mut img1, &horizon)?;
    blackout_sky(&mut img2, &horizon)?;

    if ARGS.n == 0 {
        let d1 = fractal_dim( &img1, ARGS.min_scale)?;
        let d2 = fractal_dim( &img2, ARGS.min_scale)?;
        let d = (d2-d1).abs();

        println!("{:9.4}", d);

    } else {
        let (tile_width, tile_height) = get_dominant_tile_size( &img1, ARGS.n, !ARGS.vertical, ARGS.keep_ratio);
        let (nx,ny) = get_grid_dim( &img1, tile_width, tile_height, ARGS.fractional_tiles);
        let mask = Mask::maybe_open_checked( ARGS.mask.as_ref(), nx, ny)?;
        let mut d1: TileData<f32> = TileData::new( nx, ny);
        let mut d2: TileData<f32> = TileData::new( nx, ny);

        process_subimage_tiles_mut( &mut img1, tile_width, tile_height, ARGS.fractional_tiles, mask.as_ref(), |sub,p| {
            if let Ok(fd_factor) = fractal_dim_of_tile( sub, ARGS.min_scale) {
                d1.set( p.0, p.1, fd_factor);
            }
        })?;

        process_subimage_tiles_mut( &mut img2, tile_width, tile_height, ARGS.fractional_tiles, mask.as_ref(), |sub,p| {
            if let Ok(fd_factor) = fractal_dim_of_tile( sub, ARGS.min_scale) {
                d2.set( p.0, p.1, fd_factor);
            }
        })?;

        let diff = d1.abs_diff( &d2)?;

        diff.print( 6, 2);
    }

    Ok(())
 }