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

use std::path::{Path, PathBuf};
use odin_gdal::{gdal::raster::ResampleAlg, Buffer, Dataset, DriverManager, SpatialRef};
use odin_dem::{raycast::NO_DATA_HEIGHT, build_dem_vrt, encode_dem_file, encoding::{DemSourceInfo, F32_NODATA}, DemBlockCache, DemEncoding, EncodeOptions};

const N: usize = 1200;          // pixels per source tile
const RES: f64 = 1.0 / 1200.0;  // 3 arcsec
const SRC_NODATA: f32 = -999999.0;

struct TestDir(PathBuf);
impl TestDir {
    fn new (name: &str)->Self {
        let path = std::env::temp_dir().join( format!("odin_dem_test_{name}_{}", std::process::id()));
        std::fs::create_dir_all( &path).unwrap();
        TestDir(path)
    }
}
impl Drop for TestDir {
    fn drop (&mut self) { let _ = std::fs::remove_dir_all( &self.0); }
}

/// synthetic Float32 source height at (lon,lat), None for NoData
fn height (lon: f64, lat: f64)->Option<f64> {
    if lon > -121.1 && lat > 37.9 { return None } // NoData corner (e.g. ocean outside of coverage)
    Some( -50.0 + 1700.0 * (0.5 + 0.5 * (lon * 7.0).sin() * (lat * 5.0).cos()) + 0.0137 * ((lon * 1000.0).fract() * 97.0))
}

/// create a 1x1 deg source tile in the original 3DEP format (Float32, NoData -999999, strips) with SW corner at (lon0,lat0)
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

fn create_sources (dir: &Path)->Vec<PathBuf> {
    let src_dir = dir.join("src");
    std::fs::create_dir_all( &src_dir).unwrap();
    [(-123.0, 37.0), (-122.0, 37.0), (-123.0, 38.0), (-122.0, 38.0)].iter().map( |(lon,lat)| {
        let path = src_dir.join( format!("src_{}_{}.tif", -lon, lat));
        create_source( &path, *lon, *lat);
        path
    }).collect()
}

fn check_encoding (enc: DemEncoding) {
    let td = TestDir::new( &format!("enc_{enc}"));
    let sources = create_sources( &td.0);
    let out_dir = td.0.join( enc.to_string());
    std::fs::create_dir_all( &out_dir).unwrap();

    let opts = EncodeOptions::default();
    let mut encoded = Vec::new();
    for src in &sources {
        let dst = out_dir.join( src.file_name().unwrap());
        let stats = encode_dem_file( src, &dst, enc, &opts).unwrap();
        println!("{enc} {:?}: {} -> {} bytes, max err {:.4}m, {} nodata, {} clamped", dst.file_name().unwrap(),
                 stats.src_bytes, stats.dst_bytes, stats.max_error, stats.n_nodata, stats.n_clamped);
        assert!( stats.max_error <= enc.max_error() + 1e-4);
        assert_eq!( stats.n_clamped, 0);

        let info = DemSourceInfo::read( &dst).unwrap();
        assert_eq!( info.encoding, Some(enc));
        assert_eq!( info.block_size, (512,512));
        assert_eq!( info.n_overviews, 5); // 4,8,16,32,64
        assert_eq!( info.nodata, Some( enc.nodata()));
        encoded.push( dst);
    }
    assert!( !out_dir.join("src_122_38.tif.part").exists());

    // the VRT has to return meters, independent of encoding
    let vrt_path = out_dir.join("dem.vrt");
    let stats = build_dem_vrt( &encoded, &vrt_path, enc).unwrap();
    assert!( stats.warnings.is_empty(), "{:?}", stats.warnings);
    assert_eq!( (stats.width, stats.height), (2*N, 2*N));
    assert_eq!( stats.files.len(), if enc == DemEncoding::U16 { 3 } else { 1 });

    let mut dem = DemBlockCache::open( &vrt_path, 64).unwrap();
    for (lon, lat) in [(-122.5, 37.5), (-121.37, 37.21), (-122.01, 38.73), (-121.999, 37.999), (-121.05, 38.5)] {
        // pixel center, so that bilinear interpolation returns the pixel value
        let (lon, lat) = (-123.0 + (((lon + 123.0) / RES).floor() + 0.5) * RES, 39.0 - (((39.0 - lat) / RES).floor() + 0.5) * RES);
        let h = dem.height_at( lon, lat).unwrap();
        match height( lon, lat) {
            Some(expected) => {
                let h = h.unwrap();
                println!("  {lon:.5},{lat:.5}: {h:.3} (expected {expected:.3})");
                assert!( (h - expected).abs() <= enc.max_error() + 1e-3);
            }
            None => assert_eq!( h, Some(NO_DATA_HEIGHT), "expected NoData at {lon},{lat}") // DemBlockCache maps NoData
        }
    }

    // resampled read of the whole VRT (uses source overviews)
    let ds = Dataset::open( &vrt_path).unwrap();
    let band = ds.rasterband(1).unwrap();
    let t0 = std::time::Instant::now();
    let buf: Buffer<f64> = band.read_as( (0,0), (2*N, 2*N), (75,75), Some(ResampleAlg::Bilinear)).unwrap();
    println!("  resampled 75x75 read: {:.1}ms", t0.elapsed().as_secs_f64() * 1000.0);
    let nodata = band.no_data_value().unwrap();
    let data = buf.data();
    let n_nodata = data.iter().filter( |v| **v == nodata).count();
    assert!( n_nodata > 0 && n_nodata < data.len() / 10, "{n_nodata} nodata pixels");
    for v in data.iter().filter( |v| **v != nodata) {
        assert!( *v > -60.0 && *v < 1700.0, "invalid resampled height {v}");
    }
    if enc != DemEncoding::I16 { assert_eq!( nodata, F32_NODATA as f64); }
}

#[test]
fn test_u16 () { check_encoding( DemEncoding::U16); }

#[test]
fn test_i16 () { check_encoding( DemEncoding::I16); }

#[test]
fn test_f32 () { check_encoding( DemEncoding::F32); }

#[test]
fn test_clamping_and_errors () {
    let td = TestDir::new("enc_errors");
    let src = td.0.join("high.tif");
    let driver = DriverManager::get_driver_by_name("GTiff").unwrap();
    let mut ds = driver.create_with_band_type::<f32,_>( &src, 64, 64, 1).unwrap();
    ds.set_geo_transform( &[-122.0, RES, 0.0, 38.0, 0.0, -RES]).unwrap();
    ds.set_spatial_ref( &SpatialRef::from_epsg(4269).unwrap()).unwrap();
    let data: Vec<f32> = (0..64*64).map( |i| if i < 10 { 7000.0 } else if i < 15 { -600.0 } else { 100.0 }).collect();
    ds.rasterband(1).unwrap().write( (0,0), (64,64), &mut Buffer::new( (64,64), data)).unwrap();
    drop(ds);

    let dst = td.0.join("high_u16.tif");
    let stats = encode_dem_file( &src, &dst, DemEncoding::U16, &EncodeOptions::default()).unwrap();
    assert_eq!( stats.n_clamped, 15); // above 6053.5m and below -499.9m
    assert_eq!( stats.n_nodata, 0);

    // encoding mismatch
    assert!( build_dem_vrt( &[dst.clone()], td.0.join("x.vrt"), DemEncoding::F32).is_err());
    assert!( build_dem_vrt( &[src.clone()], td.0.join("x.vrt"), DemEncoding::U16).is_err());
    assert!( build_dem_vrt( &[], td.0.join("x.vrt"), DemEncoding::U16).is_err());

    // F32 VRT of a source without overviews works but warns
    let stats = build_dem_vrt( &[src], td.0.join("f32.vrt"), DemEncoding::F32).unwrap();
    println!("warnings: {:?}", stats.warnings);
    assert!( stats.warnings.len() == 1 && stats.warnings[0].contains("no overviews"));
}
