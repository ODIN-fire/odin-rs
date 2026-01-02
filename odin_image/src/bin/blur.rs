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
use image::imageops::GaussianBlurParameters;
use odin_common::define_cli;
use odin_image::blur;

define_cli! { ARGS [about="draw tile boundaries on image"] =
   kernel_size: usize [help="Gaussian blur kernel size (3,5,7)", long, short, default_value="3"],
   src_file: String [help="filename of input image"],
   tgt_file: String [help="filename of output image"]
}

fn main() -> Result<()> {
    let input_img = image::open(&ARGS.src_file)?;
    let output_img = blur( &input_img, blur_parameters())?;
    output_img.save(&ARGS.tgt_file)?;

    Ok(())
}

fn blur_parameters ()->GaussianBlurParameters {
    match ARGS.kernel_size {
        7 => GaussianBlurParameters::SMOOTHING_7,
        5 => GaussianBlurParameters::SMOOTHING_5,
        _ => GaussianBlurParameters::SMOOTHING_3
    }
}