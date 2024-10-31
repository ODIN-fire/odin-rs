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
#![allow(unused)]

use odin_dem::*;
use odin_common:: {define_cli, fs::{self, ensure_writable_dir}, geo::BoundingBox};
use std::path::{Path,PathBuf};

define_cli! { ARGS [about="get_dem - retrieve DEM file from given GDAL VRT"] =
    west:  f64 [help="west boundary (degrees or meters depending on target SRS)", allow_hyphen_values = true, long, short],
    south: f64 [help="south boundary", allow_hyphen_values = true, long, short],
    east:  f64 [help="east boundary", allow_hyphen_values = true, long, short],
    north: f64 [help="north boundary", allow_hyphen_values = true, long, short],

    img_type: String [help="image type to create {png,tif}", short,long,default_value="tif"],
    epsg: u32 [help="target SRS EPSG number for returned DEM (also has to be used for bounding box)", long,default_value="32610"],
    output_dir: String [help="directory where to store extracted file",long,short, default_value="."],
    vrt_file: String [help="path to GDAL *.vrt file to create the DEM from"]
}

fn main() {
    odin_build::set_bin_context!();

    let out_dir = Path::new( &ARGS.output_dir).to_path_buf();
    ensure_writable_dir(&out_dir).expect("output dir does not exist or is not writable");

    // we use the generic BoundingBox instead of GeoBoundingBox since the values depend on the target SRS 
    let bbox = BoundingBox::<f64>::new( ARGS.west, ARGS.south, ARGS.east, ARGS.north);

    if let Some(img_type) = DemImgType::for_ext(ARGS.img_type.as_str()) {
        if fs::existing_non_empty_file_from_path(&ARGS.vrt_file).is_ok() {
            let dem_srs = DemSRS::from_epsg( ARGS.epsg).expect("unsupported EPSG");
            let dem_img = DemImgType::for_ext( &ARGS.img_type).expect("unsupported DEM image type");

            match get_dem( &bbox, dem_srs, dem_img, ARGS.vrt_file.as_str(), &out_dir) {
                Ok((file_path, file)) => println!("DEM file at {}", file_path),
                Err(e) => eprintln!("failed to create DEM file, error: {}", e)
            }
        } else { eprintln!("VRT file not found {}", ARGS.vrt_file) }
    } else { eprintln!("unknown target image type {}", ARGS.img_type) }
}