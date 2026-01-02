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
use odin_image::{create_luma8_image, create_luma16_image, create_luma32f_image};
use anyhow::{Result, anyhow};

define_cli! { ARGS [about="histogram equalize RGB image"] =
    fmt: String [help="pixel format (u8,u16,f32)", long, short, default_value="u8"],
    src_file: String [help="filename of image source to convert to luma8"],
    tgt_file: String [help="filename of grayscale output image"]
}

fn main()->Result<()> {
   match ARGS.fmt.as_str() {
      "u8" => Ok( create_luma8_image( &ARGS.src_file, &ARGS.tgt_file)? ),
      "u16" => Ok( create_luma16_image( &ARGS.src_file, &ARGS.tgt_file)? ),
      "f32" => Ok( create_luma32f_image( &ARGS.src_file, &ARGS.tgt_file)? ),
      _ => Err( anyhow!(format!("unknown target format {} (must be u8|u16|f32)", ARGS.fmt)))
   }
}