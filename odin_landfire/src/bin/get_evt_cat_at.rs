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

use tokio;
use anyhow::{Result,anyhow};

use odin_common::{define_cli, geo::GeoPoint};
use odin_gdal::geods::{GeoDs,GeoDsSrs};
use odin_landfire::{load_config, get_evt_value_at_geo_point, evt_cats::{EVT_CATEGORIES, EvtCategories}};

define_cli! { ARGS [about="get fuel categories for position"] =
    lf_src: Option<String> [help="file name for Landfire GeoDs config", long],
    cat_src: Option<String> [help="pathname of category file to use", long],
    latitude: f64  [help="latitude [degrees)", allow_hyphen_values = true],
    longitude: f64 [help="longitude [degrees]", allow_hyphen_values = true]
}

#[tokio::main]
async fn main ()->Result<()> {

    let pos = GeoPoint::from_lon_lat_degrees( ARGS.longitude, ARGS.latitude);

    let lf_src: GeoDs = if let Some(src) = &ARGS.lf_src {
        load_config( src)?
    } else {
        load_config( "lf_2025_conus.ron")?  // our default file source
    };

    let v = get_evt_value_at_geo_point( &lf_src, &pos).await?;

    if let Some(cat_src) = &ARGS.cat_src {
        let cats = EvtCategories::load_from( cat_src)?;
        show_cats_at( &cats, &pos, v);
    } else {
        show_cats_at( &EVT_CATEGORIES, &pos, v);
    }

    Ok(())
}

fn show_cats_at (cats: &EvtCategories, pos: &GeoPoint, v: usize) {
    println!("EVT index value: {}", v);

    if cats.is_tree( v) { println!("tree") }
    if cats.is_shrub(v) { println!("shrub") }
    if cats.is_herb(v)  { println!("herb") }
    if cats.is_agriculture(v) { println!("agriculture")}
    if cats.is_developed(v) {println!("developed")}
    if cats.is_sparse(v) {println!("sparse")}
    if cats.is_barren(v) {println!("barren")}
    if cats.is_water(v) {println!("water")}

    if cats.is_tree_dominated(v) { println!("tree dominated")}
    if cats.is_shrub_dominated(v) { println!("shrub dominated")}
    if cats.is_herb_dominated(v) { println!("herb dominated")}
}
