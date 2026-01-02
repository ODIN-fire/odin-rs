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

use anyhow::Result;
use image::Rgb;
use odin_common::define_cli;
use odin_image::{draw_tile_grid, get_hex_rgb, get_dominant_tile_size};

define_cli! { ARGS [about="draw tile boundaries on image"] =
    n: usize [help="number of tiles", long, short, default_value="10"],
    vertical: bool [help="tile vertically",long],
    keep_ratio: bool [help="use aspect-ratio tile sizes (default is square)",long],
    font_size: f32 [help="font point size to use", long, default_value="13"],
    color: String [help="color for grid lines and text", long, default_value="00ffff"],
    src_file: String [help="filename of input image"],
    tgt_file: String [help="filename of output image"]
}

fn main() -> Result<()> {
    let img = image::open(&ARGS.src_file)?;

    let (tile_width, tile_height) = get_dominant_tile_size( &img, ARGS.n, !ARGS.vertical, ARGS.keep_ratio);
    let clr = Rgb(get_hex_rgb(&ARGS.color));

    let mut img = img.to_rgb8();
    draw_tile_grid(&mut img, tile_width, tile_height, ARGS.font_size, clr, None)?;
    img.save(&ARGS.tgt_file)?;

    Ok(())
}