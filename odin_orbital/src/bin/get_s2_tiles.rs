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

use std::{fs,num::NonZero};
use kiddo::{KdTree, dist::SquaredEuclidean};
use rkyv::{ from_bytes, {rancor::Error as RkyvError}};
use anyhow::{anyhow,Result};

use odin_common::{define_cli,cartographic::Cartographic,cartesian3::Cartesian3};
use odin_orbital::copernicus::{TileKdTree,tile_dec};

define_cli! { ARGS [about="get Sentinel-2 tile ids for given longitude and latitude"] =
    input: String [help="filename for (rkyv) serialized kd-tree input"],
    lon: f64 [help="longitude in degrees", allow_hyphen_values=true, long],
    lat: f64 [help="latitude in degrees", allow_hyphen_values=true, long],
    radius: Option<f64> [help="optional radius in km to search", long]
}

fn main()->Result<()> {
    let bytes = fs::read( &ARGS.input)?;
    let kdtree = from_bytes::<TileKdTree,RkyvError>(&bytes)?;

    let geo = Cartographic::from_degrees( ARGS.lon, ARGS.lat, 0.0);
    let ecef: Cartesian3 = geo.into();

    if let Some(radius) = ARGS.radius {
        let dist = (radius * 1000.0).powi(2);
        println!("tiles within {} km of lon={}, lat={}:", radius, ARGS.lon, ARGS.lat);

        let nearest = kdtree
            .query( &[ecef.x, ecef.y, ecef.z])
            .best_n_within::<SquaredEuclidean<f64>>( dist, NonZero::new(64 as usize).unwrap())
            .execute();

        for nn in nearest.iter() {
            let tile_id = tile_dec(nn.item);
            println!("  {}", tile_id);
        }

    } else {
        let nearest = kdtree
            .query( &[ecef.x, ecef.y, ecef.z])
            .nearest_one::<SquaredEuclidean<f64>>()
            .execute();

        let tile_id = tile_dec(nearest.item);
        println!("result for lon={}, lat={} : {}", ARGS.lon, ARGS.lat, tile_id);
    }

    Ok(())
}
