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
use ndarray::Array2;
use anyhow::{Result};
use odin_image::{avg_horizontal_brenner_focus, get_grid_dim, get_dominant_tile_size, process_subimage_tiles_mut, Mask};

define_cli! { ARGS [about="quantify average blur factor of image tiles"] =
   fractional_tiles: bool [help="process fractional tiles at right/bottom image", long],
   n: usize [help="number of tiles", long, short, default_value="10"],
   horizontal: bool [help="tile horizontally",long],
   keep_ratio: bool [help="use aspect-ratio tile sizes (default is square)",long],
   mask_file: Option<String> [help="optional mask file to use", short, long],
   src_file: String [help="filename of image to analyze"]
}

fn main()->Result<()> {
   let mut img = image::open( &ARGS.src_file)?;
   let (tile_width, tile_height) = get_dominant_tile_size( &img, ARGS.n, ARGS.horizontal, ARGS.keep_ratio);
   let (nx,ny) = get_grid_dim( &img, tile_width, tile_height, ARGS.fractional_tiles);
   let mask = Mask::maybe_open_checked( ARGS.mask_file.as_ref(), nx, ny)?;
   let mut focus = Array2::<f32>::zeros((nx, ny));

   process_subimage_tiles_mut( &mut img, tile_width, tile_height, ARGS.fractional_tiles, mask.as_ref(), |sub,p| {
      focus[[p.0, p.1]] = avg_horizontal_brenner_focus( sub) as f32;
   })?;

   for j in 0..ny {
      for i in 0..nx {
         print!("{:9.4}", focus[[i,j]]);
      }
      println!("");
   }

    Ok(())
}