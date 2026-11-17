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

//! tests for the odin_nexrad data model. Decoding of a real Level II volume is tested if ODIN_NEXRAD_SAMPLES points
//! to a directory with `KMUX20261116_235440_V06`. Tests that access the AWS S3 bucket only run if ODIN_NEXRAD_S3 is set

use std::{path::{Path, PathBuf}, time::Duration};
use chrono::{DateTime, NaiveDate, TimeZone, TimeDelta, Utc};
use odin_nexrad::{*, product::*, store::*};

fn t (h: u32, m: u32, s: u32)->DateTime<Utc> { Utc.with_ymd_and_hms( 2026, 11, 17, h, m, s).unwrap() }

fn volume (h: u32, m: u32, s: u32, delay_secs: i64)->VolumeInfo {
    let scan_time = t(h,m,s);
    VolumeInfo {
        station: "KMUX".into(), scan_time, key: format!("2026/11/17/KMUX/KMUX{}_V06", scan_time.format("%Y%m%d_%H%M%S")),
        size: 9_000_000, last_modified: Some( scan_time + TimeDelta::seconds( delay_secs))
    }
}

#[test]
fn test_config () {
    let cfg: NexradConfig = ron::from_str( include_str!("../configs/nexrad.ron")).unwrap();
    assert_eq!( cfg.bucket, DEFAULT_BUCKET);
    assert_eq!( cfg.products, vec![ NexradProduct::BaseReflectivity, NexradProduct::CompositeReflectivity ]);
    assert_eq!( cfg.check_delay, Duration::from_secs(30));
    assert_eq!( cfg.retry_delay, Duration::from_secs(60));
    assert!( cfg.stations.contains( &"KMUX".to_string()));
    assert_eq!( NexradProduct::from_name("cref"), Some( NexradProduct::CompositeReflectivity));
}

#[test]
fn test_volume_names () {
    assert_eq!( parse_volume_name("KMUX20261117_001157_V06"), Some(("KMUX".into(), t(0,11,57))));
    assert_eq!( parse_volume_name("KMUX20261117_001157_V06.gz"), Some(("KMUX".into(), t(0,11,57))));
    assert_eq!( parse_volume_name("KMUX20261117_001157"), Some(("KMUX".into(), t(0,11,57)))); // old volumes without version
    assert_eq!( parse_volume_name("KMUX20261116_235440_V06_MDM"), None); // model data
    assert_eq!( parse_volume_name("KMUX2026111_001157_V06"), None);
    assert_eq!( parse_volume_name("NWS_NEXRAD_..."), None);
    assert_eq!( day_prefix( "KMUX", NaiveDate::from_ymd_opt( 2026, 11, 7).unwrap()), "2026/11/07/KMUX/");

    let v = volume( 0, 11, 57, 350);
    assert_eq!( v.name(), "KMUX20261117_001157_V06");
    assert_eq!( v.publication_delay(), Some( Duration::from_secs(350)));
}

#[test]
fn test_timing () {
    // intervals 5:46, 5:45, 5:56 (out of order), delays 5:40..5:50
    let vs = vec![ volume(0,0,26,340), volume(0,11,57,350), volume(0,6,11,339), volume(0,17,53,337) ];
    let tm = VolumeTiming::from_volumes( &vs).unwrap();
    assert_eq!( tm.count, 4);
    assert_eq!( tm.first_scan, t(0,0,26));
    assert_eq!( tm.last_scan, t(0,17,53));
    assert_eq!( tm.last_interval, Some( Duration::from_secs(356)));
    assert_eq!( tm.interval_min, Some( Duration::from_secs(345)));
    assert_eq!( tm.interval_median, Some( Duration::from_secs(346)));
    assert_eq!( tm.interval_max, Some( Duration::from_secs(356)));
    assert_eq!( tm.delay_min, Some( Duration::from_secs(337)));
    assert_eq!( tm.delay_max, Some( Duration::from_secs(350)));
    assert_eq!( tm.next_expected( Duration::ZERO, Duration::ZERO), t(0,17,53) + TimeDelta::seconds( 356 + 340));

    let single = VolumeTiming::from_volumes( &[ VolumeInfo{ last_modified: None, ..volume(0,0,26,0) }]).unwrap();
    assert!( single.last_interval.is_none() && single.delay_median.is_none());
    assert_eq!( single.next_expected( Duration::from_secs(300), Duration::from_secs(360)), t(0,0,26) + TimeDelta::seconds(660));
    assert!( VolumeTiming::from_volumes( &[]).is_none());
}

#[test]
fn test_reflectivity_encoding () {
    assert_eq!( dbz_to_code( -32.0), 2);
    assert_eq!( dbz_to_code( 0.0), 66);
    assert_eq!( dbz_to_code( 35.5), 137);
    assert_eq!( dbz_to_code( 94.5), 255);
    assert_eq!( dbz_to_code( -40.0), 2);  // clamped
    assert_eq!( dbz_to_code( 120.0), 255);
    assert_eq!( code_to_dbz( 137), Some(35.5));
    assert_eq!( code_to_dbz( REF_BELOW_THRESHOLD), None);
    assert_eq!( code_to_dbz( REF_RANGE_FOLDED), None);
    for code in 2..=255u8 { assert_eq!( dbz_to_code( code_to_dbz( code).unwrap()), code); } // lossless for 0.5dBZ values
}

#[test]
fn test_stations () {
    let kmux = get_station( "KMUX", &[]).unwrap(); // from nexrad crate registry
    assert!( (kmux.latitude - 37.155).abs() < 0.01 && (kmux.longitude + 121.898).abs() < 0.01);
    assert!( kmux.name.contains("San Francisco"));
    assert!( get_station( "XXXX", &[]).is_none());

    let path = PathBuf::from( env!("CARGO_TARGET_TMPDIR")).join( "nexrad_stations.csv");
    std::fs::write( &path, "index,X,Y,OBJECTID,siteID,siteName,radarType,antennaElevation\n\
                            0,-98.41304599968552,45.45583300028834,1,KABR,ABERDEEN,NEXRAD,Unknown\n\
                            1,127.0,37.0,2,RKSG,CAMP HUMPHREYS,NEXRAD,52\n").unwrap();
    let csv = read_stations_csv( &path).unwrap();
    assert_eq!( csv.len(), 2);
    assert_eq!( csv[0].elevation_m, None);
    let rksg = get_station( "RKSG", &csv).unwrap(); // not in registry
    assert_eq!( (rksg.latitude, rksg.longitude, rksg.elevation_m), (37.0, 127.0, Some(52.0)));
}

#[test]
fn test_entries () {
    let e = NexradEntry::new( Path::new("/c"), "KMUX", NexradProduct::BaseReflectivity, t(0,11,57));
    assert_eq!( e.layer_name(), "KMUX_bref_20261117T001157Z");
    assert_eq!( e.path, PathBuf::from("/c/KMUX/bref/KMUX_bref_20261117T001157Z.tif"));
    assert_eq!( parse_date_tag("20261117T001157Z"), Some( t(0,11,57)));
    assert_eq!( volume_path( Path::new("/c"), &volume(0,11,57,0)), PathBuf::from("/c/KMUX/volumes/KMUX20261117_001157_V06"));
}

/// decode a real volume (set ODIN_NEXRAD_SAMPLES to a directory with KMUX20261116_235440_V06)
#[test]
fn test_decode_sample () {
    let Ok(dir) = std::env::var("ODIN_NEXRAD_SAMPLES") else { println!("ODIN_NEXRAD_SAMPLES not set - skipped"); return };
    let data = std::fs::read( Path::new(&dir).join("KMUX20261116_235440_V06")).unwrap();
    let scan = decode_volume( data).unwrap();
    let site = scan.site().unwrap();
    assert_eq!( site.identifier(), "KMUX");
    let (idx, field) = scan.first_field( nexrad::model::data::Product::Reflectivity).unwrap();
    assert_eq!( idx, 0);
    assert!( field.max_range_km() > 400.0);
    // all valid reflectivity values are on the 0.5dBZ grid of our 8 bit encoding
    for v in field.values() { if v.is_finite() { assert_eq!( code_to_dbz( dbz_to_code( *v)), Some( v.clamp( MIN_DBZ, MAX_DBZ)), "{v}") } }
}

/// list volumes of the S3 bucket (set ODIN_NEXRAD_S3 to run)
#[tokio::test]
async fn test_s3_listing () {
    if std::env::var("ODIN_NEXRAD_S3").is_err() { println!("ODIN_NEXRAD_S3 not set - skipped"); return }
    let client = odin_common::s3::create_s3_client( DEFAULT_REGION).await.unwrap();
    let date = NaiveDate::from_ymd_opt( 2026, 11, 16).unwrap();
    let vs = list_volumes( &client, DEFAULT_BUCKET, "KMUX", date, None).await.unwrap();
    assert!( vs.len() > 100); // ~250 volumes per day
    assert!( vs.windows(2).all( |w| w[0].scan_time < w[1].scan_time));
    assert!( vs.iter().all( |v| v.station == "KMUX" && v.last_modified.is_some()));

    let after = list_volumes( &client, DEFAULT_BUCKET, "KMUX", date, Some( &vs[vs.len()-3].key)).await.unwrap(); // incremental listing
    assert_eq!( after.len(), 2);

    let stations = list_stations( &client, DEFAULT_BUCKET, date).await.unwrap();
    assert!( stations.len() > 150 && stations.contains( &"KMUX".to_string()));
}

fn station (id: &str)->Station { get_station( id, &[]).unwrap() }

#[test]
fn test_store () {
    let mut store = NexradStore::new( vec![ station("KMUX"), station("KDAX")], &NexradProduct::ALL, 3);
    let e = |p: NexradProduct, m: u32| NexradEntry::new( Path::new("/c"), "KMUX", p, t(0,m,0));
    let both = |m: u32| vec![ e( NexradProduct::BaseReflectivity, m), e( NexradProduct::CompositeReflectivity, m)];

    for m in [10, 20, 15] { // out of order
        let u = store.insert( "KMUX", both(m));
        assert_eq!( (u.added.len(), u.removed.len()), (2, 0));
    }
    assert!( store.insert( "KMUX", both(15)).is_empty()); // duplicates
    assert!( store.insert( "XXXX", both(30)).is_empty()); // unknown station

    let u = store.insert( "KMUX", both(25)); // drops oldest of each product
    assert_eq!( u.added, both(25));
    assert_eq!( u.removed, both(10));

    let u = store.insert( "KMUX", both(5)); // older than all we keep
    assert!( u.added.is_empty());
    assert_eq!( u.removed, both(5));

    assert_eq!( store.scan_times("KMUX"), vec![ t(0,15,0), t(0,20,0), t(0,25,0)]);
    assert!( store.scan_times("KDAX").is_empty());
    let se = store.station("KMUX").unwrap();
    assert_eq!( se.latest_scan(), Some( t(0,25,0)));
    assert!( se.has_scan( &t(0,20,0)) && !se.has_scan( &t(0,10,0)));

    let json = store.json_value();
    assert_eq!( json["products"][1]["name"], "cref");
    assert_eq!( json["stations"][0]["id"], "KMUX");
    assert_eq!( json["stations"][0]["products"][0]["entries"][0]["layer"], "KMUX_bref_20261117T002500Z"); // newest first
    assert_eq!( json["stations"][0]["extent"].as_array().unwrap().len(), 4);

    let u = NexradUpdate { station: "KMUX".into(), added: both(30), removed: both(15) }.json_value();
    assert_eq!( u["added"][1]["product"], "cref");
    assert_eq!( u["removed"][0], "KMUX_bref_20261117T001500Z");
}

#[test]
fn test_cache_rebuild () {
    let dir = PathBuf::from( env!("CARGO_TARGET_TMPDIR")).join( "nexrad_cache");
    let _ = std::fs::remove_dir_all( &dir);
    for m in [5, 10, 15] {
        let path = entry_path( &dir, "KMUX", NexradProduct::BaseReflectivity, &t(0,m,0));
        std::fs::create_dir_all( path.parent().unwrap()).unwrap();
        std::fs::write( &path, b"x").unwrap();
    }
    let pdir = dir.join("KMUX").join("bref");
    std::fs::write( pdir.join("KMUX_bref_20261117T002000Z.tmp"), b"x").unwrap(); // incomplete
    std::fs::write( pdir.join("junk.tif"), b"x").unwrap();

    let store = load_cached_store( vec![ station("KMUX")], &NexradProduct::ALL, 2, &dir);
    assert_eq!( store.scan_times("KMUX"), vec![ t(0,10,0), t(0,15,0)]);
    let mut files: Vec<String> = std::fs::read_dir( &pdir).unwrap().map( |de| de.unwrap().file_name().into_string().unwrap()).collect();
    files.sort();
    assert_eq!( files, vec!["KMUX_bref_20261117T001000Z.tif", "KMUX_bref_20261117T001500Z.tif"]);
}

#[test]
fn test_palette () {
    let palette = reflectivity_palette( 5.0);
    assert_eq!( palette.len(), 256);
    assert_eq!( palette[0][3], 0);                         // below threshold
    assert_eq!( palette[1][3], 0);                         // range folded
    assert_eq!( palette[ dbz_to_code(4.5) as usize][3], 0); // below min_dbz
    assert_eq!( palette[ dbz_to_code(5.0) as usize], [0,255,255,255]);   // nexrad-render scale: 5dBZ cyan
    assert_eq!( palette[ dbz_to_code(52.5) as usize], [255,48,48,255]);  // 50dBZ red

    let cmap = reflectivity_cmap( 5.0);
    assert!( cmap[0].is_nodata && cmap[ dbz_to_code(4.5) as usize].is_nodata && !cmap[ dbz_to_code(5.0) as usize].is_nodata);
    assert_eq!( cmap[ dbz_to_code(35.5) as usize].name, "35.5 dBZ");
}

/// compute products of a real volume (set ODIN_NEXRAD_SAMPLES to a directory with KMUX20261116_235440_V06)
#[test]
fn test_compute_products () {
    let Ok(dir) = std::env::var("ODIN_NEXRAD_SAMPLES") else { println!("ODIN_NEXRAD_SAMPLES not set - skipped"); return };
    let data = std::fs::read( Path::new(&dir).join("KMUX20261116_235440_V06")).unwrap();
    let scan = decode_volume( data).unwrap();
    let cache_dir = PathBuf::from( env!("CARGO_TARGET_TMPDIR")).join( "nexrad_products");
    let _ = std::fs::remove_dir_all( &cache_dir);

    let t0 = std::time::Instant::now();
    let entries = compute_products( &scan, &station("KMUX"), &NexradProduct::ALL, t(0,0,0), 0.005, 5.0, &cache_dir).unwrap();
    println!("computed {} products in {:?}", entries.len(), t0.elapsed());
    assert_eq!( entries.len(), 2);

    for e in &entries {
        let ds = gdal::Dataset::open( &e.path).unwrap();
        let (w, h) = ds.raster_size();
        let gt = ds.geo_transform().unwrap();
        assert_eq!( ds.spatial_ref().unwrap().auth_code().unwrap(), 4326);
        assert!( (gt[1] - 0.005).abs() < 1e-9 && (gt[5] + 0.005).abs() < 1e-9);
        // grid is centered on KMUX (37.155,-121.898) and covers ~460km
        let (lon_c, lat_c) = (gt[0] + gt[1] * w as f64 / 2.0, gt[3] + gt[5] * h as f64 / 2.0);
        assert!( (lon_c + 121.898).abs() < 0.05 && (lat_c - 37.155).abs() < 0.05, "center {lat_c},{lon_c}");
        assert!( h as f64 * 0.005 > 8.0);

        let band = ds.rasterband(1).unwrap();
        assert!( band.overview_count().unwrap() >= 3);
        let codes = band.read_as::<u8>( (0,0), (w,h), (w,h), None).unwrap();
        let n_valid = codes.data().iter().filter( |c| **c >= 2).count();
        println!("{}: {w}x{h}, {n_valid} valid cells, {} bytes", e.layer_name(), std::fs::metadata( &e.path).unwrap().len());
        assert!( n_valid > 1000);
    }

    // composite >= base everywhere (base is one of the composite sweeps)
    let read = |e: &NexradEntry| { let ds = gdal::Dataset::open( &e.path).unwrap(); let (w,h) = ds.raster_size(); ds.rasterband(1).unwrap().read_as::<u8>( (0,0), (w,h), (w,h), None).unwrap().data().to_vec() };
    let (base, comp) = (read( &entries[0]), read( &entries[1]));
    if base.len() == comp.len() {
        assert!( base.iter().zip( comp.iter()).all( |(b,c)| *b < 2 || *c >= *b));
    }

    // WMS layer with value lookup
    let cfg: NexradConfig = ron::from_str( include_str!("../configs/nexrad.ron")).unwrap();
    let wms = service::NexradWms::new( &cfg);
    wms.add_entry( &entries[0]).unwrap();
    let layer = wms.wms.layer( &entries[0].layer_name()).unwrap();
    let fv = layer.feature_value( odin_gdal::geods::GeoDsSrs::WGS84, -121.898, 37.155).unwrap(); // radar position
    println!("value at radar position: {fv:?}");
}
