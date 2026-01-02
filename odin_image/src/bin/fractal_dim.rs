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

use anyhow::{Result};
use odin_common::define_cli;
use odin_image::{process_subimage_tiles_mut, fractal_dim, fractal_dim_of_tile, get_dominant_tile_size, get_grid_dim, Mask, TileData};

define_cli! { ARGS [about="estimate fractal dimension of given image"] =
    min_scale: f32 [help="minimum scale factor [0..1]", long, default_value="0.4"],
    fractional_tiles: bool [help="process fractional tiles at right/bottom image", long],
    n: usize [help="number of tiles", long, short, default_value="10"],
    vertical: bool [help="tile vertically",long],
    keep_ratio: bool [help="use aspect-ratio tile sizes (default is square)",long],
    mask_file: Option<String> [help="optional mask file to use", short, long],
    fm_file: Option<String> [help="optional file name of fractal dimension factor map",short,long],
    src_file: String [help="filename of image to estimate"]
}

fn main() -> Result<()> {
    let mut img = image::open(&ARGS.src_file)?;

    if ARGS.n == 0 {
        let d = fractal_dim(&img, ARGS.min_scale)?;
        println!("{d}");

    } else {
        let (tile_width, tile_height) = get_dominant_tile_size( &img, ARGS.n, !ARGS.vertical, ARGS.keep_ratio);
        let (nx,ny) = get_grid_dim( &img, tile_width, tile_height, ARGS.fractional_tiles);
        let mask = Mask::maybe_open_checked( ARGS.mask_file.as_ref(), nx, ny)?;
        let mut td: TileData<f32> = TileData::new( nx, ny);

        process_subimage_tiles_mut( &mut img, tile_width, tile_height, ARGS.fractional_tiles, mask.as_ref(), |sub,p| {
            if let Ok(v) = fractal_dim_of_tile( sub, ARGS.min_scale) {
                td.set( p.0, p.1, v);
            }
        })?;

        println!("fractal dimension");
        td.print( 6, 2);

        if let Some(path) = &ARGS.fm_file {
            td.save(path)?;
        }
    }

    Ok(())
}
