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
use image;
use odin_common::define_cli;

define_cli! { ARGS [about="crop image"] =
   x: u32 [help="left origin in pixels", long, short],
   y: u32 [help="top origin in pixels", long, short],
   width: u32 [help="width in pixels", long, short],
   height: u32 [help="height in pixels", long, short],
   src_file: String [help="filename of input image"],
   tgt_file: String [help="filename of output image"]
}

fn main() -> Result<()> {
    let input_img = image::open(&ARGS.src_file)?;
    let output_img = input_img.crop_imm( ARGS.x, ARGS.y, ARGS.width, ARGS.height);
    output_img.save(&ARGS.tgt_file)?;

    Ok(())
}