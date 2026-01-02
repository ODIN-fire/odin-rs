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

use image::{DynamicImage, GenericImageView};
use odin_common::define_cli;
use odin_image::{
    get_dominant_tile_size, get_grid_dim, hsv_horizon_line, load_checked_horizon, open_diff_image_pair, get_horizon,
    smoke::{get_gw_smoke_diff, SmokeDiff}, Mask, TileData, {Result as OdinImageResult}
};
use anyhow::{Result, anyhow};

define_cli! { ARGS [about="compute diffs of gray/white tile factors for two images"] =
    top_margin: u32 [help="optional top margin to crop from input image", long, default_value="0"],
    bottom_margin: u32 [help="optional bottom margin to crop from input image", long, default_value="0"],

    //--- tile grid parameters
    fractional_tiles: bool [help="process fractional tiles at right/bottom image", long],
    n: usize [help="number of tiles", long, short, default_value="10"],
    vertical: bool [help="tile vertically",long],
    keep_ratio: bool [help="use aspect-ratio tile sizes (default is square)",long],
    mask: Option<String> [help="optional mask file to use (JSON)", long],

    //--- horizon detection parameters
    y_dist: u32 [help="horizontal distance in pixels to determine V,S gradients", long, default_value="20"],
    v_diff: f32 [help="min V difference (gradient) we consider to be the horizon line [0..-1]", long, default_value="-0.1"],
    s_diff: f32 [help="min S difference (gradient) we consider to be the horizon line [0..1]", long, default_value="0.1"],
    loess_width: usize [help="bandwidth for LOESS smoothing of horizon edge", long, default_value="20"],
    offset: i32 [help="horizon offset in pixels", long, allow_hyphen_values = true, default_value="0"],
    horizon: Option<String> [help="optional horizon file to use (JSON)", long],

    //--- smoke detection parameters
    min_g_diff: f32 [help="minimum threshold for gray-ness difference", long, default_value="0.0"],
    min_w_diff: f32 [help="minimum threshold for white-ness difference", long, default_value="0.02"],
    w_saturation: f32 [help="threshold for sky saturation", long, default_value="0.97"],
    min_valid: f32 [help="required minimum fraction of non-filtered pixels per tile [0..1]", long, default_value="0.3"],

    tgt_file: Option<String> [help="optional filename for annotated smoke image", long],
    src_file1: String [help="filename of first image to compare"],
    src_file2: String [help="filename of second image to compare"]
}

fn main()->Result<()> {
    let (img1, img2) = open_diff_image_pair(&ARGS.src_file1, &ARGS.src_file2)?;

    let (w,h) = img1.dimensions();
    let (tile_width, tile_height) = get_dominant_tile_size( &img1, ARGS.n, !ARGS.vertical, ARGS.keep_ratio);
    let (nx,ny) = get_grid_dim( &img1, tile_width, tile_height, ARGS.fractional_tiles);
    let mask = Mask::maybe_open_checked( ARGS.mask.as_ref(), nx, ny)?;
    let horizon = get_horizon( ARGS.horizon.as_ref(), &img1, ARGS.top_margin, ARGS.y_dist, ARGS.v_diff, ARGS.s_diff, ARGS.loess_width)?;

    let smoke_cells = get_gw_smoke_diff( &img1, &img2, tile_width, tile_height, 
        ARGS.fractional_tiles, mask.as_ref(), 
        ARGS.min_valid, ARGS.min_g_diff, ARGS.min_w_diff, ARGS.w_saturation,
        horizon.as_slice()
    )?;
    if !smoke_cells.is_empty() {
        println!("             g_diff   w_diff");
        for sd in &smoke_cells {
            let i = sd.x / tile_width;
            let j = sd.y / tile_height;
            println!("[{:2},{:2}] : {:9.3}{:9.3}", i,j, sd.g_diff, sd.w_diff);
        }
    }

    Ok(())
}
