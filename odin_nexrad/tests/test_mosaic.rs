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

//! tests for the overview mosaic computation (with synthetic station grids)

use std::{sync::Arc, time::Duration};
use chrono::{DateTime, TimeZone, Utc};
use odin_nexrad::{*, product::*, mosaic::*};

fn t (h: u32, m: u32, s: u32)->DateTime<Utc> { Utc.with_ymd_and_hms( 2026, 11, 18, h, m, s).unwrap() }

fn station (id: &str, lat: f64, lon: f64)->Station {
    Station { id: id.into(), name: id.into(), latitude: lat, longitude: lon, elevation_m: None }
}

/// a reduced station grid with a constant value and 1km cells
fn const_grid (s: &Station, range_m: f64, value: f32)->Arc<StationGrid> {
    let (res_lon, res_lat) = cell_size_deg( s.latitude, 1000.0);
    let dlat = range_m / METERS_PER_DEG_LAT;
    let dlon = dlat * res_lon / res_lat;
    let grid = ProductGrid { west: s.longitude - dlon, north: s.latitude + dlat, res_lon, res_lat,
                             width: (2.0 * dlon / res_lon).ceil() as usize, height: (2.0 * dlat / res_lat).ceil() as usize };
    let dbz = vec![ value; grid.width * grid.height];
    Arc::new( StationGrid { station: s.id.clone(), scan_time: t(12,0,0), latitude: s.latitude, longitude: s.longitude, max_range_m: range_m, grid, dbz })
}

#[test]
fn test_reduce () {
    let s = station( "KTST", 37.0, -122.0);
    let (res_lon, res_lat) = cell_size_deg( s.latitude, 250.0);
    let grid = ProductGrid { west: -122.0, north: 37.0, res_lon, res_lat, width: 8, height: 8 };

    let mut codes = vec![ REF_BELOW_THRESHOLD; 64];
    codes[0] = dbz_to_code( 50.0);                                  // one 50 dBZ cell in the first 4x4 block
    for r in 0..4 { for c in 4..8 { codes[r*8 + c] = dbz_to_code( 30.0) } } // uniform block
    for r in 4..8 { for c in 0..3 { codes[r*8 + c] = REF_RANGE_FOLDED } }  // mostly range folded block

    let sg = StationGrid::from_codes( &s, t(12,0,0), &codes, &grid, 1000.0, 5.0).unwrap();
    assert_eq!( (sg.grid.width, sg.grid.height), (2, 2));
    println!("reduced: {:?}", sg.dbz);

    // linear Z mean: 50 dBZ in 1/16 of the area is 50 - 10*log10(16) = 37.96 dBZ
    assert!( (sg.dbz[0] - 37.96).abs() < 0.05);
    assert!( (sg.dbz[1] - 30.0).abs() < 0.01);
    assert!( sg.dbz[2].is_nan());
    assert_eq!( sg.dbz[3], 5.0); // no echo
}

#[test]
fn test_blend () {
    // two stations 200km apart (east-west) with constant values
    let a = station( "KAAA", 37.0, -122.0);
    let b = station( "KBBB", 37.0, -122.0 + 200_000.0 / (METERS_PER_DEG_LAT * 37f64.to_radians().cos()));
    let grids = vec![ const_grid( &a, 300_000.0, 20.0), const_grid( &b, 300_000.0, 40.0) ];

    let grid = mosaic_grid( &[a.clone(), b.clone()], 1000.0).unwrap();
    let values = blend( &grids, &grid, 20_000.0, 5.0);

    let value_at = |lon: f64| values[ grid.cell_index( 37.0, lon).unwrap()];
    let x = |km: f64| a.longitude + km * 1000.0 / (METERS_PER_DEG_LAT * 37f64.to_radians().cos());

    assert!( (value_at( x(50.0)) - 20.0).abs() < 1e-3);    // only nearest station (outside of transition zone)
    assert!( (value_at( x(150.0)) - 40.0).abs() < 1e-3);
    assert!( (value_at( x(100.0)) - 30.0).abs() < 0.5);    // equal weights at the boundary
    let v1 = value_at( x(95.0));                           // transition: 10km difference -> weights 1 : 0.5
    assert!( v1 > 20.0 && v1 < 30.0 && (v1 - 26.67).abs() < 0.7, "{v1}");

    // monotonic across the transition zone (no seams)
    let profile: Vec<f32> = (80..=120).map( |km| value_at( x( km as f64))).collect();
    assert!( profile.windows(2).all( |w| w[1] >= w[0] - 1e-3), "{profile:?}");

    // outside of both ranges
    assert_eq!( value_at( x(-299.0) - 0.05), 5.0);
}

#[test]
fn test_smooth () {
    let (w, h) = (21, 21);
    let mut values = vec![ 0.0f32; w * h];
    values[10*w + 10] = 100.0;
    smooth( &mut values, w, h, 2.0);

    let sum: f32 = values.iter().sum();
    assert!( (sum - 100.0).abs() < 0.1, "{sum}"); // preserves mass (no edge effects)
    assert!( values[10*w + 10] < 5.0);
    assert!( (values[10*w + 8] - values[8*w + 10]).abs() < 1e-4); // isotropic
    assert!( (values[10*w + 8] - values[10*w + 12]).abs() < 1e-4);
}

#[test]
fn test_contour () {
    // concentric bands: a cone with 55 dBZ at the center, decreasing by 1 dBZ per km
    let s = station( "KTST", 37.0, -122.0);
    let grid = mosaic_grid( &[s.clone()], 1000.0).unwrap();
    let (cr, cc) = (grid.height / 2, grid.width / 2);
    let values: Vec<f32> = (0..grid.width * grid.height).map( |i| {
        let (r, c) = ((i / grid.width) as f64, (i % grid.width) as f64);
        let d = ((r - cr as f64).powi(2) + (c - cc as f64).powi(2)).sqrt();
        (55.0 - d as f32).max(5.0)
    }).collect();

    let levels = [10.0, 20.0, 30.0, 40.0, 50.0];
    let bands = contour( &values, &grid, &levels, 250.0, 4.0).unwrap();
    for b in &bands { println!("band {} - {:?}: {} polygons, {} vertices", b.lo, b.hi, b.polygons.len(), b.n_vertices()) }

    assert_eq!( bands.iter().map( |b| b.lo).collect::<Vec<_>>(), levels.to_vec());
    assert_eq!( bands.last().unwrap().hi, None);
    assert_eq!( bands[0].hi, Some(20.0));
    for b in &bands {
        assert_eq!( b.polygons.len(), 1);
        let n_rings = b.polygons[0].len();
        if b.hi.is_some() { assert_eq!( n_rings, 2) } else { assert_eq!( n_rings, 1) } // annulus or disk
    }

    // outer ring of the 10 dBZ band has a radius of 45km
    let ring = &bands[0].polygons[0][0];
    let (lat0, lon0) = (grid.cell_center( cr, cc).latitude, grid.cell_center( cr, cc).longitude);
    let cos_lat = lat0.to_radians().cos();
    for p in ring {
        let d = (((p[0] - lon0) * cos_lat).powi(2) + (p[1] - lat0).powi(2)).sqrt() * METERS_PER_DEG_LAT / 1000.0;
        assert!( (d - 45.0).abs() < 1.5, "{d}");
    }

    // small polygons are dropped
    let bands = contour( &values, &grid, &levels, 250.0, 100.0).unwrap();
    assert!( bands.iter().all( |b| b.lo < 50.0)); // the 50+ disk has a radius of 5km (~78km²)

    // GeoJSON
    let gj = bands_geojson( &bands, &reflectivity_palette( 5.0));
    let features = gj["features"].as_array().unwrap();
    assert_eq!( features.len(), bands.len());
    assert_eq!( features[0]["properties"]["lo"], 10.0);
    assert_eq!( features[0]["geometry"]["type"], "MultiPolygon");
    assert!( features[0]["properties"]["color"].as_str().unwrap().starts_with('#'));
}

#[test]
fn test_inputs () {
    let mk = |id: &str, h: u32, m: u32| NexradEntry::new( std::path::Path::new("/tmp"), id, NexradProduct::BaseReflectivity, t(h,m,0));
    let latest = vec![ mk("KAAA", 12, 0), mk("KBBB", 12, 10), mk("KCCC", 11, 50) ];
    let inputs = mosaic_inputs( latest.iter(), Duration::from_secs( 15 * 60));
    assert_eq!( inputs.iter().map( |e| e.station.as_str()).collect::<Vec<_>>(), vec!["KAAA", "KBBB"]);
    assert!( mosaic_inputs( std::iter::empty(), Duration::from_secs(60)).is_empty());
}

fn mosaic (product: NexradProduct, date: DateTime<Utc>, dir: &std::path::Path)->NexradMosaic {
    NexradMosaic { product, date, stations: vec![ ("KAAA".into(), date)], path: dir.join( mosaic_file_name( product, &date)),
                   n_polygons: 0, n_vertices: 0, compute_time: Duration::ZERO }
}

#[test]
fn test_store_mosaics () {
    use odin_nexrad::store::NexradStore;
    let dir = std::path::Path::new("/tmp");
    let (bref, cref) = (NexradProduct::BaseReflectivity, NexradProduct::CompositeReflectivity);
    let mut store = NexradStore::new( vec![], &[bref, cref], 12);

    assert!( store.add_mosaic( mosaic( bref, t(12,0,0), dir), 2).removed.is_empty());
    assert!( store.add_mosaic( mosaic( cref, t(12,0,0), dir), 2).removed.is_empty());
    assert!( store.add_mosaic( mosaic( bref, t(12,5,0), dir), 2).removed.is_empty());

    // same date replaces (not reported as removed)
    let mut m = mosaic( bref, t(12,5,0), dir);
    m.stations.push( ("KBBB".into(), t(12,3,0)));
    assert!( store.add_mosaic( m, 2).removed.is_empty());
    assert_eq!( store.product_mosaics( bref).count(), 2);
    assert_eq!( store.product_mosaics( bref).last().unwrap().stations.len(), 2);

    // dropping the oldest one of the same product
    let update = store.add_mosaic( mosaic( bref, t(12,10,0), dir), 2);
    assert_eq!( update.removed.iter().map( |m| m.date).collect::<Vec<_>>(), vec![ t(12,0,0)]);
    assert_eq!( store.product_mosaics( bref).map( |m| m.date).collect::<Vec<_>>(), vec![ t(12,5,0), t(12,10,0)]);
    assert_eq!( store.product_mosaics( cref).count(), 1);

    let v = update.json_value();
    assert_eq!( v["removed"][0], t(12,0,0).timestamp_millis());
    assert_eq!( v["date"], t(12,10,0).timestamp_millis());
}

#[test]
fn test_cached_mosaics () {
    let dir = std::env::temp_dir().join( format!("test_cached_mosaics_{}", std::process::id()));
    let _ = std::fs::remove_dir_all( &dir);
    std::fs::create_dir_all( &dir).unwrap();
    let bref = NexradProduct::BaseReflectivity;

    let gj = serde_json::json!({ "type": "FeatureCollection",
        "features": [{ "type": "Feature", "properties": { "lo": 10.0, "hi": null, "color": "#000000" },
                       "geometry": { "type": "MultiPolygon", "coordinates": [[[[0.0,0.0],[1.0,0.0],[1.0,1.0],[0.0,0.0]]]] }}],
        "stations": [{ "id": "KAAA", "date": t(12,0,0).timestamp_millis() }]
    }).to_string();
    for m in 0..4 { std::fs::write( dir.join( mosaic_file_name( bref, &t(12,m*5,0))), &gj).unwrap(); }
    std::fs::write( dir.join( mosaic_file_name( bref, &t(13,0,0))), "garbage").unwrap();
    std::fs::write( dir.join( "bref_mosaic_x.tmp"), "").unwrap();

    let mosaics = load_cached_mosaics( &dir, &[bref], 3);
    assert_eq!( mosaics.iter().map( |m| m.date).collect::<Vec<_>>(), vec![ t(12,5,0), t(12,10,0), t(12,15,0)]);
    assert_eq!( mosaics[0].stations, vec![ ("KAAA".to_string(), t(12,0,0))]);
    assert_eq!( (mosaics[0].n_polygons, mosaics[0].n_vertices), (1, 4));
    assert_eq!( std::fs::read_dir( &dir).unwrap().count(), 3); // dropped, unreadable and tmp files are removed

    std::fs::remove_dir_all( &dir).unwrap();
}
