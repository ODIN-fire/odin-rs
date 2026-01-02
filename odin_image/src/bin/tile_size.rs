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
use image::GenericImageView;
use odin_common::define_cli;
use odin_image::{get_dominant_tile_size, get_grid_dim};

define_cli! { ARGS [about="compute tile size in pixels for given image"] =
    fractional_tiles: bool [help="process fractional tiles at right/bottom image", long],
    n: usize [help="number of tiles", long, short, default_value="10"],
    vertical: bool [help="tile vertically",long],
    keep_ratio: bool [help="use aspect-ratio tile sizes (default is square)",long],
    src_file: String [help="image filename"]
}

fn main() -> Result<()> {
    let img = image::open(&ARGS.src_file)?;

    if ARGS.n == 0 { // this is not too useful
        let (w,h) = img.dimensions();
        println!("width:  {w}");
        println!("height: {h}");

    } else {
        let (w, h) = get_dominant_tile_size( &img, ARGS.n, !ARGS.vertical, ARGS.keep_ratio);
        let (nx,ny) = get_grid_dim( &img, w, h, ARGS.fractional_tiles);

        println!("width:  {w}, nx: {nx}");
        println!("height: {h}, ny: {ny}");
    }

    Ok(())
}