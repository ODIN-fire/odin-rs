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

use std::{path::{Path,PathBuf}};
use odin_common::{define_cli, BoundingBox};

define_cli! { ARGS [about="crop provided GDAL raster file so that it does not contain NO_DATA values"] =
    input: String [help="input filename"],
    output: String [help="output filename"]
}

fn main()->Result<()> {
    Ok(())
}