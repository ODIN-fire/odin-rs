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
 use odin_image::rgb_equalize;
 use anyhow::{Result};

 define_cli! { ARGS [about="histogram equalize RGB image"] =
    src_file: String [help="filename of image source to equalize"],
    tgt_file: String [help="filename of equalized output image"]
 }

 fn main()->Result<()> {
    Ok( rgb_equalize( &ARGS.src_file, &ARGS.tgt_file)? )
 }