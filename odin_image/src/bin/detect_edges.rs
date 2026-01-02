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
use anyhow::Result;
use image::{DynamicImage,GenericImageView,ImageBuffer};
use edge_detection::canny;
use odin_common::define_cli;

define_cli! { ARGS [about="detect edges using the Canny algorithm"] =
   top_margin: u32 [help="optional top margin to crop", long, default_value="0"],
   bottom_margin: u32 [help="optional bottom margin to crop", long, default_value="0"],
   sigma: f32 [help="sigma (small: fine edges,more noise)", long, default_value="1.2"],
   strong: f32 [help="strong threshold [0..1.0]", long, default_value="0.2"],
   weak: f32 [help="weak threshold [0..1.0] < strong_threshold", long, default_value="0.01"],
   src_file: String [help="filename of input image"],
   tgt_file: String [help="filename of output (edge) image"]
}

fn main() -> Result<()> {
    let mut img = image::open(&ARGS.src_file)?;
    let (w,h) = img.dimensions();

    let luma8_img = if ARGS.top_margin > 0 || ARGS.bottom_margin > 0 {
        let cropped_img = img.crop_imm(0, ARGS.top_margin, img.width(), h - ARGS.top_margin - ARGS.bottom_margin);
        cropped_img.to_luma8()
    } else {
        img.to_luma8()
    };

    let detection = canny( luma8_img, ARGS.sigma, ARGS.strong, ARGS.weak);

    let edge_img = detection.as_image();
    edge_img.save(&ARGS.tgt_file)?;

    Ok(())
}
