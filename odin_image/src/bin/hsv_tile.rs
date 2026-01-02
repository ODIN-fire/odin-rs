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

use anyhow::{Result};
use image::{DynamicImage,GenericImageView, SubImage};
use odin_common::define_cli;
use odin_image::{get_dominant_tile_size, get_grid_dim, tiled_mean_hsv, hsv_horizon_line, load_checked_horizon, mean_hsv, process_subimage_tiles, Mask, TileData};
use odin_image::{Result as OdinImageResult};

define_cli! { ARGS [about="compute average HSV values of tiles from given image"] =
    top_margin: u32 [help="optional top margin to crop from input image", long, default_value="0"],
    bottom_margin: u32 [help="optional bottom margin to crop from input image", long, default_value="0"],

    //--- tile grid parameters
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

    //--- tile data constraints
    v_saturation: f32 [help="threshold for sky saturation", long, default_value="0.97"],
    min_valid: f32 [help="required minimum fraction of non-filtered pixels per tile [0..1]", long, default_value="0.3"],

    mask: Option<String> [help="optional mask file to use", long],
    horizon: Option<String> [help="optional filename for horizon (JSON) file", long],
    src_file: String [help="filename of image to analyze"]
}

fn main() -> Result<()> {
    let img = image::open(&ARGS.src_file)?;
    let horizon = get_horizon( &img)?;

    let valid_pixel_pred = |x: u32, y: u32, hsv: &(f32,f32,f32)| {
        (y >= horizon[x as usize]) && hsv.2 < ARGS.v_saturation
    };

    if ARGS.n == 0 { // this is not too useful
        let (w,h) = img.dimensions();
        let img = img.view(0, 0, w, h);
        match mean_hsv(&img, ARGS.min_valid, valid_pixel_pred) {
            Ok( (h,s,v) ) => {
                println!("H: {:.2}", h);
                println!("S: {:.2}", s);
                println!("V: {:.2}", v);
            }
            Err(e) => {
                println!("{e}");
            }
        }

    } else {
        let (tile_width, tile_height) = get_dominant_tile_size( &img, ARGS.n, !ARGS.vertical, ARGS.keep_ratio);
        let (nx,ny) = get_grid_dim( &img, tile_width, tile_height, ARGS.fractional_tiles);
        let mask = Mask::maybe_open_checked( ARGS.mask.as_ref(), nx, ny)?;

        let (h_data, s_data, v_data) = tiled_mean_hsv( 
            &img, tile_width, tile_height, ARGS.fractional_tiles, mask.as_ref(), 
            ARGS.min_valid, &valid_pixel_pred
        )?;

        println!("Hue:");
        h_data.print( 8, 2);

        println!();
        println!("Saturation:");
        s_data.print( 6, 2);

        println!();
        println!("Value:");
        v_data.print( 6, 2);
    }

    Ok(())
}

fn get_horizon (img: &DynamicImage)->OdinImageResult<Vec<u32>> {
    if let Some(path) = &ARGS.horizon {
        load_checked_horizon(path, img.width())
    } else {
        hsv_horizon_line( &img, ARGS.top_margin, ARGS.y_dist, ARGS.v_diff, ARGS.s_diff, ARGS.loess_width)
    }
}