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

use image_compare::{Algorithm};

use odin_common::define_cli;
use odin_image::{gray_structure_compare};
use anyhow::{Result};

const DEFAULT_ALGORITHM: Algorithm = Algorithm::MSSIMSimple;

define_cli! { ARGS [about="histogram structure compare"] =
    algorithm: Option<String> [help="algorithm to use (rms,mssim)", long, short, default_value="mssim"],
    img1: String [help="first input image"],
    img2: String [help="second input image"],
    out_file: Option<String> [help="optional diff visualization image filename"]
}

fn main()->Result<()> {
    let alg = get_algorithm();
    let img1 = image::open( &ARGS.img1)?;
    let img2 = image::open( &ARGS.img2)?;

    let similarity = gray_structure_compare( &img1, &img2, alg)?;
    println!("similarity score: {}", similarity.score);

    if let Some(diff_path) = &ARGS.out_file {
       let diff_img = similarity.image.to_color_map();
       diff_img.save( diff_path)?;
   }

    Ok(())
}

fn get_algorithm()->Algorithm {
    if let Some(m) = &ARGS.algorithm {
        match m.as_str() {
            "rms" => Algorithm::RootMeanSquared,
            "mssim" => Algorithm::MSSIMSimple,
            _ => {
                println!("unknown algorithm, falling back to default");
                DEFAULT_ALGORITHM
            }
        }
    } else {
        DEFAULT_ALGORITHM
    }
}