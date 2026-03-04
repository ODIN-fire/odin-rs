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
use std::{fs,collections::HashSet};
use kiddo::{KdTree, NearestNeighbour, SquaredEuclidean};
use rkyv::{ from_bytes, {rancor::Error as RkyvError}};
use odin_build::pkg_data_dir;
use uom::si::{f64::Length, length::meter};
use crate::{errors::Result,overpass::Overpass};

pub fn tile_enc (tile_id: &str)->u64 {
    let tc = tile_id.as_bytes();

    let tile_enc: u64 =
        tc[0] as u64
        | (tc[1] as u64) << 8
        | (tc[2] as u64) << 16
        | (tc[3] as u64) << 32
        | (tc[4] as u64) << 40;
    tile_enc
}

pub fn tile_dec (tile_enc: u64)->String {
    let bs: [u8;5] = [
        (tile_enc & 0xff) as u8,
        ((tile_enc >> 8) & 0xff) as u8,
        ((tile_enc >> 16) & 0xff) as u8,
        ((tile_enc >> 32) & 0xff) as u8,
        ((tile_enc >> 40) & 0xff) as u8
    ];

    String::from_utf8_lossy(&bs).into()
}

pub fn load_kdtree ()->Result<KdTree<f64,3>> {
    let path = pkg_data_dir!().join("s2a-tiles.rkyv");
    let bytes = fs::read( &path)?;
    Ok( from_bytes::<KdTree<f64,3>,RkyvError>(&bytes)? )
}

pub fn get_overpass_tiles (kdtree: &KdTree<f64,3>, overpass: &Overpass, radius: Length)->Vec<String> {
    let mut set: HashSet<String> = HashSet::new();
    let dist = radius.get::<meter>().powi(2);

    let track = &overpass.track;
    for ecef in track {
        for nn in kdtree.best_n_within::<SquaredEuclidean>( &[ecef.x, ecef.y, ecef.z], dist, 64) {
            let tile_id = tile_dec(nn.item);
            set.insert( tile_id);
        }
    }

    let mut tiles = Vec::from_iter(set);
    tiles.sort();

    tiles
}
