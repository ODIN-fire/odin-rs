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
#![cfg(feature="terrain")]

use std::{io::Read, path::{Path, PathBuf}};
use odin_common::fs::EnvPathBuf;
use odin_gdal::{Buffer, DriverManager, SpatialRef};
use odin_dem::{build_dem_vrt, encode_dem_file, DemEncoding, EncodeOptions};
use odin_cesium::terrain::*;

const N: usize = 1200;          // pixels per source tile
const RES: f64 = 1.0 / 1200.0;  // 3 arcsec
const SRC_NODATA: f32 = -999999.0;
const GEOID_PATH: &str = "/opt/homebrew/share/proj/us_nga_egm08_25.tif";

struct TestDir(PathBuf);
impl TestDir {
    fn new (name: &str)->Self {
        let path = std::env::temp_dir().join( format!("odin_cesium_test_{name}_{}", std::process::id()));
        std::fs::create_dir_all( &path).unwrap();
        TestDir(path)
    }
}
impl Drop for TestDir {
    fn drop (&mut self) { let _ = std::fs::remove_dir_all( &self.0); }
}

/// synthetic source height, None for NoData (ocean)
fn height (lon: f64, lat: f64)->Option<f64> {
    if lon < -122.8 && lat < 37.5 { return None }
    Some( 20.0 + 1500.0 * (0.5 + 0.5 * (lon * 3.0).sin() * (lat * 2.0).cos()))
}

fn create_source (path: &Path, lon0: f64, lat0: f64) {
    let driver = DriverManager::get_driver_by_name("GTiff").unwrap();
    let mut ds = driver.create_with_band_type::<f32,_>( path, N, N, 1).unwrap();
    ds.set_geo_transform( &[lon0, RES, 0.0, lat0 + 1.0, 0.0, -RES]).unwrap();
    ds.set_spatial_ref( &SpatialRef::from_epsg(4269).unwrap()).unwrap();
    let mut data = Vec::with_capacity( N*N);
    for iy in 0..N {
        for ix in 0..N {
            let (lon, lat) = (lon0 + (ix as f64 + 0.5) * RES, lat0 + 1.0 - (iy as f64 + 0.5) * RES);
            data.push( height( lon, lat).map( |h| h as f32).unwrap_or( SRC_NODATA));
        }
    }
    let mut band = ds.rasterband(1).unwrap();
    band.set_no_data_value( Some( SRC_NODATA as f64)).unwrap();
    band.write( (0,0), (N,N), &mut Buffer::new( (N,N), data)).unwrap();
}

/// 2x2 deg U16 DEM VRT [-123,-121] x [37,39]
fn create_vrt (dir: &Path)->PathBuf {
    let mut encoded = Vec::new();
    for (lon, lat) in [(-123.0, 37.0), (-122.0, 37.0), (-123.0, 38.0), (-122.0, 38.0)] {
        let src = dir.join( format!("src_{}_{lat}.tif", -lon));
        create_source( &src, lon, lat);
        let dst = dir.join( format!("u16_{}_{lat}.tif", -lon));
        encode_dem_file( &src, &dst, DemEncoding::U16, &EncodeOptions::default()).unwrap();
        encoded.push( dst);
    }
    let vrt = dir.join("dem.vrt");
    build_dem_vrt( &encoded, &vrt, DemEncoding::U16).unwrap();
    vrt
}

fn decode (gz: &[u8])->(Vec<f64>,u8) {
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new( gz).read_to_end( &mut raw).unwrap();
    assert_eq!( raw.len(), TILE_SIZE * TILE_SIZE * 2 + 1);
    let hs = raw[..TILE_SIZE*TILE_SIZE*2].chunks(2).map( |b| u16::from_le_bytes([b[0],b[1]]) as f64 / 5.0 - 1000.0).collect();
    (hs, raw[raw.len()-1])
}

fn config (dir: &Path, vrt: &Path, geoid: Option<&str>)->TerrainConfig {
    TerrainConfig {
        vrt_path: Some( EnvPathBuf( vrt.to_path_buf())),
        geoid_path: geoid.map( |p| EnvPathBuf( PathBuf::from(p))),
        global_levels: 5,
        max_level: None,
        seed_level: None,
        cache_dir: Some( EnvPathBuf( dir.join("cache"))),
    }
}

#[test]
fn test_tiling_scheme () {
    assert_eq!( tile_bounds( 0, 0, 0), (-180.0, -90.0, 0.0, 90.0));
    assert_eq!( tile_bounds( 1, 3, 1), (90.0, 0.0, 180.0, 90.0));
    assert_eq!( level_for_resolution( 1.0/1200.0), 12);  // 3 arcsec
    assert_eq!( level_for_resolution( 1.0/10800.0), 15); // 1/3 arcsec 3DEP
    assert_eq!( TileRange::for_bounds( 2, -180.0, -90.0, -135.0, -45.0), Some( TileRange{x0:0,y0:0,x1:0,y1:0}));
    assert_eq!( parse_tile_path( "12/345/678.terrain"), Some( (12,345,678)));
    assert_eq!( parse_tile_path( "12/345/678.png"), None);
    assert_eq!( parse_tile_path( "../1/2.terrain"), None);
}

#[test]
fn test_terrain_tiles () {
    let td = TestDir::new("terrain");
    let vrt = create_vrt( &td.0);
    let tiles = TerrainTiles::new( &config( &td.0, &vrt, None)).unwrap();

    //--- availability
    let av = &tiles.availability;
    assert_eq!( av.max_level(), 12);
    assert_eq!( av.levels[0].len(), 1);
    assert!( av.is_available( 3, 0, 0)); // global levels
    for z in 6..=12 {
        let r = TileRange::for_bounds( z, -123.0, 37.0, -121.0, 39.0).unwrap();
        assert_eq!( av.levels[z as usize], vec![r], "level {z}");
    }
    assert!( !av.is_available( 6, 0, 0));
    assert_eq!( av.child_mask( 12, 0, 0), 0);
    let layer: serde_json::Value = serde_json::from_str( &tiles.layer_json()).unwrap();
    assert_eq!( layer["format"], "heightmap-1.0");
    assert_eq!( layer["available"].as_array().unwrap().len(), 13);

    //--- max level tile heights (no geoid)
    let z = 12;
    let r = av.levels[z as usize][0];
    let (x, y) = ((r.x0 + r.x1) / 2, (r.y0 + r.y1) / 2);
    let t0 = std::time::Instant::now();
    let (hs, mask) = decode( &tiles.get_tile( z, x, y).unwrap());
    println!("tile {z}/{x}/{y}: {:.1}ms", t0.elapsed().as_secs_f64()*1000.0);
    assert_eq!( mask, 0);
    let (west, _south, _east, north) = tile_bounds( z, x, y);
    let d = sample_spacing( z);
    let mut max_err: f64 = 0.0;
    for j in 0..TILE_SIZE {
        for i in 0..TILE_SIZE {
            let expected = height( west + i as f64 * d, north - j as f64 * d).unwrap_or(0.0);
            max_err = max_err.max( (hs[j*TILE_SIZE + i] - expected).abs());
        }
    }
    println!("  max error {max_err:.3}m");
    assert!( max_err < 0.5, "max error {max_err}");
    assert!( tiles.tile_path( z, x, y).is_file());

    //--- neighbouring tiles share edge samples
    let (hs_e, _) = decode( &tiles.get_tile( z, x+1, y).unwrap());
    for j in 0..TILE_SIZE {
        let (a, b) = (hs[j*TILE_SIZE + TILE_SIZE-1], hs_e[j*TILE_SIZE]);
        assert!( (a - b).abs() < 0.3, "edge mismatch at row {j}: {a} vs {b}");
    }

    //--- coarse tiles use overviews and have child masks
    for z in [0u32, 4, 7] {
        let rs = &av.levels[z as usize];
        let r = TileRange::for_bounds( z, -122.0, 38.0, -121.9, 38.1).unwrap();
        assert!( av.is_available( z, r.x0, r.y0));
        let t0 = std::time::Instant::now();
        let (hs, mask) = decode( &tiles.get_tile( z, r.x0, r.y0).unwrap());
        println!("tile {z}/{}/{}: {:.1}ms, mask {mask}, {} ranges", r.x0, r.y0, t0.elapsed().as_secs_f64()*1000.0, rs.len());
        assert!( mask != 0);
        assert!( hs.iter().all( |h| *h >= -0.2 && *h < 1550.0));
        if z > 0 { assert!( hs.iter().any( |h| *h > 100.0)); } // level 0 sample spacing (2.8deg) is larger than our DEM
    }

    //--- seeding (computes only missing tiles)
    let (n, n_err) = tiles.seed( 8, 4, |_,_| {});
    assert_eq!( n_err, 0);
    assert_eq!( n, tiles.available_tiles(8).len() - 3); // 0/0/0, 4/.. and 7/.. are already cached
    assert_eq!( tiles.seed( 8, 4, |_,_| {}).0, 0);

    //--- unavailable tile
    assert!( tiles.get_tile( 8, 0, 0).is_err());
}

#[test]
fn test_geoid () {
    if !Path::new( GEOID_PATH).is_file() { println!("no {GEOID_PATH}, skipping"); return }
    let g = GeoidGrid::load( Path::new( GEOID_PATH), 1.0/12.0).unwrap();
    // gdallocationinfo -wgs84 -valonly us_nga_egm08_25.tif -122.4 37.8  -> -32.1386
    let n = g.undulation( -122.4, 37.8);
    assert!( (n + 32.14).abs() < 0.1, "undulation {n}");
    // wrap around at the antimeridian
    assert!( (g.undulation( 179.99, 10.0) - g.undulation( -180.01, 10.0)).abs() < 0.1);

    let td = TestDir::new("geoid");
    let vrt = create_vrt( &td.0);
    let tiles = TerrainTiles::new( &config( &td.0, &vrt, Some(GEOID_PATH))).unwrap();
    let r = TileRange::for_bounds( 12, -122.41, 37.79, -122.40, 37.80).unwrap();
    let (hs, _) = decode( &tiles.get_tile( 12, r.x0, r.y0).unwrap());
    let (west, _, _, north) = tile_bounds( 12, r.x0, r.y0);
    let d = sample_spacing( 12);
    let (lon, lat) = (west + 32.0 * d, north - 32.0 * d);
    let expected = height( lon, lat).unwrap() + g.undulation( lon, lat) as f64;
    let h = hs[32*TILE_SIZE + 32];
    println!("ellipsoidal height at {lon:.4},{lat:.4}: {h:.2} (expected {expected:.2})");
    assert!( (h - expected).abs() < 0.5);
}

#[test]
fn test_availability_merge () {
    // 3x2 grid of 1x1 deg sources (with one missing), overlapping at tile borders on coarse levels
    let mut extents = Vec::new();
    for (lon, lat) in [(-123.0, 37.0), (-122.0, 37.0), (-121.0, 37.0), (-123.0, 38.0), (-122.0, 38.0)] {
        extents.push( SourceExtent{ west: lon - 0.001, south: lat - 0.001, east: lon + 1.001, north: lat + 1.001, res: 1.0/10800.0 });
    }
    let av = Availability::compute( &extents, 2, 15);
    for z in 3..=15u32 {
        let rs = &av.levels[z as usize];
        // no overlaps
        for (i, a) in rs.iter().enumerate() {
            for b in &rs[i+1..] {
                assert!( a.x1 < b.x0 || b.x1 < a.x0 || a.y1 < b.y0 || b.y1 < a.y0, "level {z}: {a:?} overlaps {b:?}");
            }
        }
        // same coverage as the source ranges
        for e in &extents {
            let r = TileRange::for_bounds( z, e.west, e.south, e.east, e.north).unwrap();
            for (x, y) in [(r.x0, r.y0), (r.x1, r.y1), (r.x0, r.y1), (r.x1, r.y0)] { assert!( av.is_available( z, x, y)) }
        }
        let (x, y) = { let r = TileRange::for_bounds( z, -120.5, 38.5, -120.4, 38.6).unwrap(); (r.x0, r.y0) };
        if z >= 9 { assert!( !av.is_available( z, x, y), "level {z}: missing source NE is available"); }
        if z == 15 { assert!( rs.len() <= 3, "{} ranges", rs.len()); }
    }
}
