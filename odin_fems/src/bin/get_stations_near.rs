/*
 * Copyright 2026 Peter Mehlitz
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://apache.org
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
#![allow(unused)]

use std::{path::{Path,PathBuf}};
use anyhow::{anyhow,Result};
use reqwest::Client;
use chrono::{Timelike, Utc};
use uom::si::f64::{Length};
use uom::si::{length::{meter,kilometer}};

use odin_common::{cartesian3::Cartesian3, cartographic::Cartographic, define_cli, geo::GeoPoint};
use odin_fems::{FemsConfig, FemsStation, FemsStationKdTree, load_config};

define_cli! { ARGS [about="download and store all FEMS station meta-data"] =
    config_name: String [help="name of config file", long, default_value="fems.ron"],
    verbose: bool [help="show metadata for stations", long, short],
    kd_path: String [help="filename for kd-tree with station ids", long, default_value="stations.rkyv"],
    lat: f64 [help="latitude in degrees", allow_hyphen_values = true],
    lon: f64 [help="longitude in degrees", allow_hyphen_values = true],
    distance: u32 [help="distance [kilometers]"]
}

fn main ()->Result<()> {
    let client = Client::new();
    let config: FemsConfig = load_config(&ARGS.config_name)?;

    let dist = Length::new::<kilometer>(ARGS.distance as f64);
    let pos = GeoPoint::from_lon_lat_degrees( ARGS.lon, ARGS.lat);
    let kd_tree = FemsStationKdTree::load_from( &ARGS.kd_path)?;
    println!("{} stations loaded", kd_tree.len());

    if ARGS.verbose {
        let stations = kd_tree.station_distances_within( &pos, dist);
        if !stations.is_empty() {
            println!("{} stations found within {} km of {}:", stations.len(), dist.get::<kilometer>(), pos);
            for (s,d) in stations.iter() {
                let s = &s.metadata;
                let pos = &s.position;
                println!("    {:8} {:20} [{:.4},{:.4}]: {:.1}km", s.id, s.name, pos.longitude_degrees(), pos.latitude_degrees(), d.get::<kilometer>() );
            }
        } else {
            println!("no stations found within {} km of {}", dist.get::<kilometer>(), pos);
        }

    } else {
        let station_ids = kd_tree.station_ids_within( &pos, dist);
        if !station_ids.is_empty() {
            println!("{} stations found within {} km of {}: {:?}", station_ids.len(), dist.get::<kilometer>(), pos, station_ids);
        } else {
            println!("no stations found within {} km of {}", dist.get::<kilometer>(), pos);
        }
    }

    Ok(())
}
