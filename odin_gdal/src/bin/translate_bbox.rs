/*
 * Copyright © 2024, United States Government, as represented by the Administrator of 
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

use gdal::spatial_ref::SpatialRef;
use odin_common::define_cli;
use odin_gdal::{transform_bounds_2d,errors::Result};

define_cli! { ARGS [about="translate axis align bounding box between spatial reference systems"] = 
    densify: Option<i32> [help="number of points to use to densify bounding polygon", long, allow_hyphen_values=true],
    s_srs: String [help="source SRS (used for min/max coordinates)", short, long],
    t_srs: String [help="target SRS (to convert to)", short, long],

    x_min: f64 [help="minimum x boundary (in source SRS)"],
    y_min: f64 [help="minimum y boundary (in source SRS)"],
    x_max: f64 [help="maximum x boundary (in source SRS)"],
    y_max: f64 [help="maximum y boundary (in source SRS)"]
}

fn main() -> Result<()> {
    let src_srs = SpatialRef::from_definition( &ARGS.s_srs.as_str())?;
    let tgt_srs = SpatialRef::from_definition( &ARGS.t_srs.as_str())?;

    println!("@@ {} {} {} {}", ARGS.x_min, ARGS.y_min, ARGS.x_max, ARGS.y_max);
    let (x_min,y_min,x_max,y_max) = transform_bounds_2d( &src_srs, &tgt_srs,
                                                         ARGS.x_min, ARGS.y_min, ARGS.x_max, ARGS.y_max,
                                                         ARGS.densify)?;

    println!(" from: '{}'", src_srs.to_proj4()?);
    println!(" to:   '{}'", tgt_srs.to_proj4()?);

    println!("  x_min:  {:15.4} -> {:10.4}", ARGS.x_min, x_min);
    println!("  y_min:  {:15.4} -> {:10.4}", ARGS.y_min, y_min);
    println!("  x_max:  {:15.4} -> {:10.4}", ARGS.x_max, x_max);
    println!("  y_max:  {:15.4} -> {:10.4}", ARGS.y_max, y_max);

    Ok(())
}