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

use std::{time::Duration, path::{Path,PathBuf}};

use odin_common::BoundingBox;
use tokio;
use ron;
use anyhow::{Result};

use odin_build;
use odin_gdal::{geods::{GeoDs, GeoDsSrs},geods_srv::run_server};

/// run with "cargo test --test test_geods test_file -- --nocapture"
#[tokio::test]
async fn test_file ()->Result<()> {
    println!("== running file-system GeoDsSource tests");
    let geo_src: GeoDs = odin_build::load_config_path( "resources/SFBay_file.ron")?;
    Ok( run_client( geo_src).await? )
}


#[tokio::test]
async fn test_server ()->Result<()> {
    println!("== running server based GeoDsSource tests");
    let geo_src: GeoDs = odin_build::load_config_path( "resources/SFBay_server.ron")?;

    let server_task = tokio::spawn( async {
        let config_path = "resources/SFBay_server_config.ron".to_string();
        println!("starting server with config {}", config_path);
        if let Err(e) = run_server( config_path).await {
            println!("server failed to start with {e}");
        }
    });

    // TODO this is not particularly reliable but we need to wait until the server is listening on the port
    println!("now waiting for server to start listening..");
    tokio::time::sleep( Duration::from_secs(1)).await;

    println!("starting client");
    run_client( geo_src).await?;
    println!("client did terminate properly");

    println!("shutting down server");
    server_task.abort();
    Ok(())
}

async fn run_client (geo_src: GeoDs)->Result<()> {
    println!("geo_src = {:?}", geo_src);

    let srs = GeoDsSrs::from_epsg(32610).unwrap(); // UTM zone 10N
    let mut locations = vec![ (594146.311, 4117378.423) ]; // easting, northing
    let values = geo_src.get_values( "slope", Some(-9999.00), &srs, &mut locations).await?;
    println!("value for UTM coords = {}", values[0]);
    assert!( values[0] == 26.0);

    let srs = GeoDsSrs::from_epsg(4326).unwrap(); // WGS84
    let mut locations = vec![ (37.198126, -121.939149) ]; // lat,lon
    let values = geo_src.get_values( "slope", Some(-9999.00), &srs, &mut locations).await?;
    println!("value for WGS84 = {}", values[0]);
    assert!( values[0] == 26.0);

    //--- grid data retrieval

    let bbox = BoundingBox {
        west: -122.037717,
        south: 37.047344,
        east: -121.809769,
        north: 37.208684
    };

    let path = Path::new("test_grid.tif").to_path_buf();
    if path.is_file() {
        std::fs::remove_file( &path)?;
    }
    geo_src.get_res_file( "slope", &bbox, srs, 0.01, 0.01, Some(-9999.0), odin_gdal::geods::GeoDsImgType::TIF, &path).await?;
    assert!( path.is_file());

    // we could check here if it is a 23x16 grid

    std::fs::remove_file( &path)?;

    geo_src.get_wh_file( "slope", &bbox, srs, 23, 16, Some(-9999.0), odin_gdal::geods::GeoDsImgType::TIF, &path).await?;
    assert!( path.is_file());

    std::fs::remove_file( &path)?;

    Ok(())
}

//--- dataset file retrieval
