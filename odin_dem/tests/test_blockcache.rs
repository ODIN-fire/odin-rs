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
use geo::{Distance, Geodesic, Point};
use odin_common::{cartographic::earth_radius_at_geodetic_latitude, fs::EnvPathBuf, geo::GeoPoint, utm::geo_to_utm, BoundingBox};
use odin_gdal::{Buffer, DriverManager, RasterCreationOptions, SpatialRef};
use odin_dem::{get_res_dem, DemBlockCache, DemGrid, DemImgType, DemSRS, DemSource, RayQuery, ResampleAlg, DEFAULT_CURV_COEFF};

const LON0: f64 = -122.0;
const LAT0: f64 = 37.5;
const M_PER_DEG: f64 = 111_320.0;

/// a scratch dir that is removed when dropped
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

/// create a tiled (128x128 blocks) geographic GeoTIFF of n x n pixels with pixel size `res` [deg] centered on
/// (LON0,LAT0). Heights are computed from the approximate (east,north) offsets [m] of pixel centers
fn create_dem (path: &Path, n: usize, res: f64, f: impl Fn(f64,f64)->f32) {
    let driver = DriverManager::get_driver_by_name("GTiff").unwrap();
    let mut opts = RasterCreationOptions::new();
    opts.add_name_value( "TILED", "YES").unwrap();
    opts.add_name_value( "BLOCKXSIZE", "128").unwrap();
    opts.add_name_value( "BLOCKYSIZE", "128").unwrap();

    let mut ds = driver.create_with_band_type_with_options::<f32,_>( path, n, n, 1, &opts).unwrap();
    let x0 = LON0 - res * n as f64 / 2.0;
    let y0 = LAT0 + res * n as f64 / 2.0;
    ds.set_geo_transform( &[x0, res, 0.0, y0, 0.0, -res]).unwrap();
    ds.set_spatial_ref( &SpatialRef::from_epsg(4326).unwrap()).unwrap();

    let cos_lat = LAT0.to_radians().cos();
    let mut data = Vec::with_capacity( n*n);
    for iy in 0..n {
        for ix in 0..n {
            let lon = x0 + (ix as f64 + 0.5) * res;
            let lat = y0 - (iy as f64 + 0.5) * res;
            data.push( f( (lon - LON0) * M_PER_DEG * cos_lat, (lat - LAT0) * M_PER_DEG));
        }
    }
    let mut band = ds.rasterband(1).unwrap();
    band.set_no_data_value( Some(-9999.0)).unwrap();
    band.write( (0,0), (n,n), &mut Buffer::new( (n,n), data)).unwrap();
}

fn origin ()->GeoPoint { GeoPoint::from_lon_lat_degrees( LON0, LAT0) }

#[test]
fn test_flat () {
    let td = TestDir::new("bc_flat");
    let path = td.0.join("flat.tif");
    create_dem( &path, 1000, 0.001, |_,_| 0.0); // ~110km x 88km
    let mut dem = DemBlockCache::open( &path, 64).unwrap();

    // no curvature: hit at h0/tan(-pitch)
    let hit = dem.ray_intersection( &origin(), 100.0, 37.0, -1.0, 20_000.0, 0.0).unwrap().unwrap();
    let expected = 100.0 / 1.0f64.to_radians().tan();
    let gd = Geodesic.distance( Point::new( LON0, LAT0), Point::new( hit.lon, hit.lat));
    println!("flat hit at {:.2}m (expected {expected:.2}m, geodesic distance {gd:.2}m), loaded {} blocks", hit.distance, dem.n_loaded_blocks());
    assert!( (hit.distance - expected).abs() < 0.01);
    assert!( (gd - hit.distance).abs() < 0.1); // the interpolated track position is on the geodesic

    // with curvature
    let k = DEFAULT_CURV_COEFF / (2.0 * earth_radius_at_geodetic_latitude( LAT0.to_radians()));
    let t = 1.0f64.to_radians().tan();
    let expected = (-t + (t*t + 4.0 * k * 100.0).sqrt()) / (2.0 * k);
    let hit = dem.ray_intersection( &origin(), 100.0, 250.0, -1.0, 20_000.0, DEFAULT_CURV_COEFF).unwrap().unwrap();
    assert!( (hit.distance - expected).abs() < 0.01);

    // horizon of a horizontal ray
    let expected = (100.0 / k).sqrt();
    let hit = dem.ray_intersection( &origin(), 100.0, 0.0, 0.0, 45_000.0, DEFAULT_CURV_COEFF).unwrap().unwrap();
    println!("horizon at {:.1}m (expected {expected:.1}m)", hit.distance);
    assert!( (hit.distance - expected).abs() < 0.1);
    let gd = Geodesic.distance( Point::new( LON0, LAT0), Point::new( hit.lon, hit.lat));
    assert!( (gd - hit.distance).abs() < 0.5);
}

#[test]
fn test_narrow_ridge () {
    let td = TestDir::new("bc_ridge");
    let path = td.0.join("ridge.tif");
    let res = 0.0001; // ~9m east-west
    let px_m = res * M_PER_DEG * LAT0.to_radians().cos();
    let ridge = 3000.0;
    create_dem( &path, 1000, res, |dx,_| if (dx - ridge).abs() <= px_m/2.0 { 200.0 } else { 0.0 });

    let mut dem = DemBlockCache::open( &path, 64).unwrap();
    let hit = dem.ray_intersection( &origin(), 100.0, 90.0, -0.5, 4_000.0, DEFAULT_CURV_COEFF).unwrap().unwrap();
    println!("ridge hit at {:.1}m, height {:.1}m", hit.distance, hit.height);
    assert!( hit.distance > ridge - 1.5 * px_m && hit.distance < ridge + px_m);
}

#[test]
fn test_lru_eviction () {
    let td = TestDir::new("bc_lru");
    let path = td.0.join("hills.tif");
    create_dem( &path, 1000, 0.0005, |dx,dy| (150.0 * (1.0 + (dx/2500.0).sin() * (dy/3100.0).cos())) as f32);

    // rays from 400m above ground that travel across several blocks before they hit (or leave the DEM)
    let queries: Vec<RayQuery> = (0..12).map( |i| RayQuery::from_ground( LON0, LAT0, 400.0, i as f64 * 30.0, -0.8, 25_000.0)).collect();

    let mut big = DemBlockCache::open( &path, 1024).unwrap();
    let mut small = DemBlockCache::open( &path, 4).unwrap();
    for q in &queries {
        let a = big.query( q).unwrap().map( |h| h.distance);
        let b = small.query( q).unwrap().map( |h| h.distance);
        println!("bearing {:5.1}: {a:?}", q.bearing);
        assert_eq!( a, b);
    }
    println!("big cache: {} loaded, {} cached; small cache: {} loaded, {} cached",
             big.n_loaded_blocks(), big.n_cached_blocks(), small.n_loaded_blocks(), small.n_cached_blocks());
    assert!( small.n_cached_blocks() <= 4);
    assert!( small.n_loaded_blocks() > big.n_loaded_blocks()); // evicted blocks had to be re-read
    assert_eq!( big.n_loaded_blocks(), big.n_cached_blocks());
}

#[tokio::test]
async fn test_dem_source_vs_grid () {
    // compare native (block cache) and UTM grid (warped at 30m) results on smooth hills
    let td = TestDir::new("bc_vs_grid");
    let path = td.0.join("hills.tif");
    create_dem( &path, 1600, 0.0002, |dx,dy| (300.0 * (1.0 + (dx/2500.0).sin() * (dy/3100.0).cos())) as f32); // ~18m pixels

    let src = DemSource::File( EnvPathBuf( path.clone()));
    let queries: Vec<RayQuery> = (0..8).map( |i| RayQuery::from_ground( LON0, LAT0, 20.0, 10.0 + i as f64 * 45.0, -1.0, 12_000.0)).collect();
    let hits = src.ray_intersections( &queries).await.unwrap();

    let o = geo_to_utm( &origin()).unwrap();
    let (x, y) = (o.easting.round(), o.northing.round());
    let bbox = BoundingBox::new( x - 13_000.0, y - 13_000.0, x + 13_000.0, y + 13_000.0);
    let grid_path = td.0.join("grid.tif");
    get_res_dem( &bbox, DemSRS::from_epsg( o.utm_zone.epsg()).unwrap(), 30.0, 30.0, ResampleAlg::Bilinear, DemImgType::TIF, &path, &grid_path).unwrap();
    let grid = DemGrid::from_file( &grid_path).unwrap();

    for (q, hit) in queries.iter().zip( hits.iter()) {
        let g = grid.query( q).unwrap();
        match (hit, &g) {
            (Some(a), Some(b)) => {
                let d = Geodesic.distance( Point::new( a.lon, a.lat), Point::new( b.lon, b.lat));
                println!("bearing {:5.1}: native {:7.1}m, grid {:7.1}m, positions {d:5.1}m apart", q.bearing, a.distance, b.distance);
                assert!( (a.distance - b.distance).abs() < 60.0); // resampling differences
                assert!( d < 60.0);
            }
            (None, None) => println!("bearing {:5.1}: no hit", q.bearing),
            _ => panic!("native and grid results differ for bearing {}: {:?} vs {:?}", q.bearing, hit, g)
        }
    }
}

#[test]
fn test_errors () {
    let td = TestDir::new("bc_errors");
    let path = td.0.join("flat.tif");
    create_dem( &path, 200, 0.001, |_,_| 50.0);
    let mut dem = DemBlockCache::open( &path, 16).unwrap();

    assert!( dem.ray_intersection( &origin(), 20.0, 0.0, -1.0, 1000.0, 0.0).is_err()); // origin below terrain
    assert!( dem.ray_intersection( &GeoPoint::from_lon_lat_degrees( -121.0, 37.5), 100.0, 0.0, -1.0, 1000.0, 0.0).is_err()); // outside
    assert!( dem.ray_intersection( &origin(), 100.0, 0.0, 1.0, 50_000.0, 0.0).unwrap().is_none()); // upwards, leaves DEM
    assert!( dem.ray_intersection( &origin(), 100.0, 0.0, -1.0, 1000.0, 0.0).unwrap().is_none()); // max_dist too short
    assert!( (dem.height_at( LON0, LAT0).unwrap().unwrap() - 50.0).abs() < 1e-6);
}
