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

use std::path::Path;
use anyhow::Result;
use clap::builder::styling::Color;
use image::{DynamicImage, GenericImage, GenericImageView, Rgb, Rgba};
use odin_common::define_cli;
use odin_image::{
    draw_tile_grid, get_aspect_ratio_tile_size, get_dominant_tile_size, get_grid_dim, get_hex_rgb, get_square_tile_size, 
    hsv_horizon_line, load_horizon, offset_horizon, save_horizon, terrain_tile_mask, 
    Mask, Result as OdinImageResult
};

define_cli! { ARGS [about="detect horizon line"] =
    top_margin: u32 [help="optional top margin to crop from input image", long, default_value="0"],
    bottom_margin: u32 [help="optional bottom margin to crop from input image", long, default_value="0"],

    //sigma: f32 [help="sigma (small: fine edges,more noise)", long, default_value="3.0"],
    //strong: f32 [help="strong threshold [0..1.0]", long, default_value="0.2"],
    //weak: f32 [help="weak threshold [0..1.0] < strong_threshold", long, default_value="0.01"],
    //threshold: f32 [help="edge magnitude threshold for horizon line [0..1]", short, long, default_value="0.2"],

    //--- horizon detection parameters
    y_dist: u32 [help="horizontal distance in pixels to determine V,S gradients", long, default_value="20"],
    v_diff: f32 [help="min V difference (gradient) we consider to be the horizon line [0..-1]", long, default_value="-0.1"],
    s_diff: f32 [help="min S difference (gradient) we consider to be the horizon line [0..1]", long, default_value="0.1"],
    loess_width: usize [help="bandwidth for LOESS smoothing of horizon edge", long, default_value="20"],
    offset: i32 [help="horizon offset in pixels", long, allow_hyphen_values = true, default_value="0"],

    //--- tile grid parameters
    n: usize [help="number of tiles", long, short, default_value="10"],
    vertical: bool [help="tile vertically",long],
    keep_ratio: bool [help="use aspect-ratio tile sizes (default is square)",long],
    fractional_tiles: bool [help="store fractional tiles in mask (only takes effect if nx,ny are specified)", long],

    mask: Option<String> [help="optional filename of mask to produce", long],
    horizon: Option<String> [help="optional horizon file to produce", long],

    font_size: f32 [help="font point size to use", long, default_value="12.0"],
    color: String [help="color for grid lines and text", long, default_value="00ffff"],
    horizon_color: String [help="hex color specification for horizon line", long, default_value="ff0000"],

    src_file: String [help="filename of input image"],
    tgt_file: String [help="filename of output image with horizon"]
}

fn main() -> Result<()> {
    let mut img = image::open(&ARGS.src_file)?;

    // this is our main purpose so we don't load an existing horizon file
    let mut horizon = hsv_horizon_line( &img, ARGS.top_margin, ARGS.y_dist, ARGS.v_diff, ARGS.s_diff, ARGS.loess_width)?;
    if ARGS.offset != 0 { 
        offset_horizon( &mut horizon, ARGS.offset, 0, img.height()-1); 
    }
    if let Some(horizon_file) = &ARGS.horizon {
        save_horizon( &horizon, horizon_file)?;
    }

    // draw the horizon line
    let rgb = get_hex_rgb(&ARGS.horizon_color);
    let horizon_color: Rgba<u8> = Rgba([rgb[0],rgb[1],rgb[2],255]);
    for x in 0..horizon.len() {
        let y = horizon[x];
        let x = x as u32;
        img.put_pixel( x, y, horizon_color);
        if y > 2 {
            img.put_pixel( x, y-1, horizon_color);
            img.put_pixel( x, y-2, horizon_color);
        }
    }
    
    // draw the mask grid
    if ARGS.n > 0 { 
        let (tile_width, tile_height) = get_dominant_tile_size( &img, ARGS.n, !ARGS.vertical, ARGS.keep_ratio);
        let mask = get_mask( &img, tile_width, tile_height, ARGS.fractional_tiles, &horizon)?;
        
        if let Some(rgb_img) = img.as_mut_rgb8() {
            let color = Rgb(get_hex_rgb(&ARGS.color));
            draw_tile_grid( rgb_img, tile_width, tile_height, ARGS.font_size, color, Some(&mask))?;
        }
    }

    img.save( &ARGS.tgt_file)?;

    Ok(())
}

fn get_mask (img: &DynamicImage, tile_width: u32, tile_height: u32, fractional_tiles: bool, horizon: &[u32])->OdinImageResult<Mask> {
    if let Some(mask_file) = &ARGS.mask {
        if Path::new(mask_file).is_file() {
            // use existing mask file but check if it has the right dimensions
            let (nx,ny) = get_grid_dim(img, tile_width, tile_height, fractional_tiles);
            return Mask::open_checked( mask_file, nx, ny)
        }
    }
        
    let mask = terrain_tile_mask( &img, tile_width, tile_height, ARGS.fractional_tiles, &horizon)?;

    if let Some(mask_file) = &ARGS.mask {
        mask.save( mask_file)?;
    }

    Ok(mask)
}
