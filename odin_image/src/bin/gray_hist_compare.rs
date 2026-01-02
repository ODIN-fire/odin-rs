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

use image_compare::{Metric};

use odin_common::define_cli;
use odin_image::{gray_histogram_compare};
use anyhow::{Result};

const DEFAULT_METRIC: Metric = Metric::Intersection;

define_cli! { ARGS [about="histogram structure compare"] =
    metric: Option<String> [help="histogram metric to use (correlation, chisquare, intersection, hellinger)", long, short, default_value="intersection"],
    img1: String [help="first input filename"],
    img2: String [help="second input filename"]
}

fn main()->Result<()> {
    let metric = get_metric();
    let img1 = image::open( &ARGS.img1)?;
    let img2 = image::open( &ARGS.img2)?;

    let sim_score = gray_histogram_compare( &img1, &img2, metric)?;
    println!("similarity score: {}", sim_score);

    Ok(())
}

fn get_metric()->Metric {
    if let Some(m) = &ARGS.metric {
        match m.as_str() {
            "correlation" => Metric::Correlation,
            "chisquare" => Metric::ChiSquare,
            "intersection" => Metric::Intersection,
            "hellinger" => Metric::Hellinger,
            _ => {
                println!("unknown metric, falling back to default");
                DEFAULT_METRIC
            }
        }
    } else {
        DEFAULT_METRIC
    }
}