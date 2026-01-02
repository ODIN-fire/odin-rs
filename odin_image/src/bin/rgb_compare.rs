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

use image::{self, GenericImageView};
use odin_common::define_cli;
use odin_image::{open_diff_image_pair, get_horizon, blackout_terrain, blackout_sky, rgb_hybrid_compare};
use anyhow::{Result};

define_cli! { ARGS [about="image differencing using MSSIM for luma and RMS for chroma channels"] =
    top_margin: u32 [help="optional top margin to crop from input image", long, default_value="0"],
    bottom_margin: u32 [help="optional bottom margin to crop from input image", long, default_value="0"],

    sky: bool [help="compare sky (otherwise compare terrain)", long],

    //--- horizon detection parameters
    y_dist: u32 [help="horizontal distance in pixels to determine V,S gradients", long, default_value="20"],
    v_diff: f32 [help="min V difference (gradient) we consider to be the horizon line [0..-1]", long, default_value="-0.1"],
    s_diff: f32 [help="min S difference (gradient) we consider to be the horizon line [0..1]", long, default_value="0.1"],
    loess_width: usize [help="bandwidth for LOESS smoothing of horizon edge", long, default_value="20"],
    offset: i32 [help="horizon offset in pixels", long, allow_hyphen_values = true, default_value="0"],
    horizon: Option<String> [help="optional horizon file to use (JSON)", long],

    diff: Option<String> [help="optional diff visualization image filename", long],

    src_file1: String [help="filename of first input image"],
    src_file2: String [help="filename of 2nd input image"]
}

fn main()->Result<()> {
    let (mut img1, mut img2) = open_diff_image_pair(&ARGS.src_file1, &ARGS.src_file2)?;

    if ARGS.top_margin > 0 || ARGS.bottom_margin > 0 {
        let (w,h) = img1.dimensions();
        img1 = img1.crop( 0, ARGS.top_margin, w, h - (ARGS.top_margin + ARGS.bottom_margin));
        img2 = img2.crop( 0, ARGS.top_margin, w, h - (ARGS.top_margin + ARGS.bottom_margin));
    }

    let horizon = get_horizon( ARGS.horizon.as_ref(), &img1, 0, ARGS.y_dist, ARGS.v_diff, ARGS.s_diff, ARGS.loess_width)?;

    if ARGS.sky {
        blackout_terrain( &mut img1, &horizon)?;
        blackout_terrain( &mut img2, &horizon)?;
    } else {
        blackout_sky( &mut img1, &horizon)?;
        blackout_sky( &mut img2, &horizon)?; 
    }

    let similarity = rgb_hybrid_compare( &img1, &img2)?;

    println!("similarity score: {}", similarity.score);

    if let Some(diff_path) = &ARGS.diff {
        let diff_img = similarity.image.to_color_map();
        diff_img.save( diff_path)?;
    }

    Ok(())
}