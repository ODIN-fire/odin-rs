/*
 * Copyright © 2026, United States Government, as represented by the Administrator of
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

use std::{fs::{File},io::Read};
use kiddo::{KdTree, NearestNeighbour, SquaredEuclidean};
use rkyv::{ from_bytes, {rancor::Error as RkyvError}};
use ron;
use uom::si::{f64::Length,length::kilometer};
use anyhow::{anyhow,Result};

use odin_common::{define_cli,cartographic::Cartographic,cartesian3::Cartesian3};
use odin_orbital::{Overpass, copernicus::{get_overpass_tiles, load_kdtree, tile_dec}};

define_cli! { ARGS [about="get Sentinel-2 tile ids for given longitude and latitude"] =
    kdtree: String [help="filename for (rkyv) serialized kd-tree input", long, default_value="s2a-tiles.rkyv"],
    radius: f64 [help="optional radius in km to search", long, default_value="150"],
    overpass: String [help="filename for (JSON) serialized overpass"]
}

fn main()->Result<()> {
    let overpass: Overpass = ron::de::from_reader( File::open(&ARGS.overpass)?)?;
    let kdtree = load_kdtree()?;

    let tiles = get_overpass_tiles( &kdtree, &overpass, Length::new::<kilometer>(ARGS.radius));
    for tile_id in tiles {
        print!("{},", tile_id);
    }
    println!();

    Ok(())
}
