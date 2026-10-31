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

//! tests for WMS layer rendering of different data types (odin_gdal::wms_srv)

use std::{collections::HashMap, path::{Path, PathBuf}};
use gdal::{DriverManager, raster::{Buffer, GdalType}, spatial_ref::SpatialRef};
use odin_common::fs::EnvPathBuf;
use odin_gdal::{
    cmap::CmapEntry, errors::Result,
    wms_srv::{WmsGetMap, WmsLayer, WmsLayerConfig, WmsRequest, WmsStyleConfig, parse_wms_request}
};

const N: usize = 100; // raster size (N x N cells of 0.01deg)
const BBOX: &str = "-122.0,37.0,-121.0,38.0";

fn tmp_path (name: &str)->PathBuf {
    PathBuf::from( env!("CARGO_TARGET_TMPDIR")).join( name)
}

/// create a N x N EPSG:4326 GeoTIFF covering BBOX
fn create_ds<T: GdalType + Copy> (path: &Path, vs: Vec<T>, nodata: Option<f64>) {
    let driver = DriverManager::get_driver_by_name("GTiff").unwrap();
    let mut ds = driver.create_with_band_type::<T,_>( path, N, N, 1).unwrap();
    ds.set_geo_transform( &[-122.0, 0.01, 0.0, 38.0, 0.0, -0.01]).unwrap();
    ds.set_spatial_ref( &SpatialRef::from_epsg(4326).unwrap()).unwrap();

    let mut band = ds.rasterband(1).unwrap();
    band.set_no_data_value( nodata).unwrap();
    band.write( (0,0), (N,N), &mut Buffer::new( (N,N), vs)).unwrap();
}

fn get_map (params: &str)->WmsGetMap {
    let q = format!("service=WMS&version=1.1.1&request=GetMap&format=image/png&layers=test&srs=EPSG:4326&bbox={BBOX}&width={N}&height={N}&{params}");
    let q: HashMap<String,String> = q.split('&').filter_map( |kv| kv.split_once('=')).map( |(k,v)| (k.to_string(), v.to_string())).collect();
    let Ok(WmsRequest::GetMap(gm)) = parse_wms_request( &q) else { panic!("not a GetMap request") };
    gm
}

fn layer_config (path: &Path, nodata: Vec<f64>, styles: Vec<WmsStyleConfig>)->WmsLayerConfig {
    WmsLayerConfig { name: "test".into(), title: "test".into(), descr: None, path: EnvPathBuf( path.to_path_buf()), nodata, styles }
}

/// render and decode PNG, returning a function to get RGBA pixel values at (col,row)
fn render (layer: &WmsLayer, gm: &WmsGetMap)->impl Fn(usize,usize)->[u8;4] + use<> {
    let style = layer.style_for( gm).expect("unknown style");
    let png = layer.render_png( style, gm).unwrap();
    let img = image::load_from_memory( &png).unwrap().to_rgba8();
    assert_eq!( img.dimensions(), (N as u32, N as u32));
    move |x,y| img.get_pixel( x as u32, y as u32).0
}

#[test]
fn test_float_alpha () {
    // values increase from west to east (0..0.99), NaN in the NW corner and declared nodata in the southern rows
    let mut vs: Vec<f32> = (0..N*N).map( |i| (i % N) as f32 / N as f32).collect();
    for y in 0..10 { for x in 0..10 { vs[y*N + x] = f32::NAN; } }
    for y in 90..N { for x in 0..N { vs[y*N + x] = -9999.0; } }

    let path = tmp_path( "wms_f32.tif");
    create_ds( &path, vs, Some(-9999.0));

    let layer = WmsLayer::new( &layer_config( &path, vec![], vec![ WmsStyleConfig::Alpha{ min: 0.0, max: 1.0 }]), None).unwrap();
    assert!( layer.lut_base.is_none()); // no value lookup tables for float data

    let px = render( &layer, &get_map( "styles=alpha&color=0000ff&ndcolor=ff00ff80"));
    assert_eq!( px(50,50), [0,0,255,128]);   // value 0.5
    assert_eq!( px(0,50), [0,0,255,0]);      // value 0.0 is transparent
    assert_eq!( px(5,5), [255,0,255,128]);   // NaN
    assert_eq!( px(50,95), [255,0,255,128]); // declared nodata

    // cmap styles require 8 or 16 bit integer data
    let reader = |_: &Path| -> Result<Vec<CmapEntry>> { Ok( vec![]) };
    let res = WmsLayer::new( &layer_config( &path, vec![], vec![ WmsStyleConfig::Cmap( EnvPathBuf( path.clone()))]), Some(&reader));
    assert!( res.is_err());
}

#[test]
fn test_byte_cmap () {
    // values 0,1,2 repeating from west to east, declared nodata 255 in the southern rows, configured nodata 3 in the NW corner
    let mut vs: Vec<u8> = (0..N*N).map( |i| ((i % N) % 3) as u8).collect();
    for y in 0..10 { for x in 0..10 { vs[y*N + x] = 3; } }
    for y in 90..N { for x in 0..N { vs[y*N + x] = 255; } }

    let path = tmp_path( "wms_u8.tif");
    create_ds( &path, vs, Some(255.0));

    let reader = |_: &Path| -> Result<Vec<CmapEntry>> {
        Ok( vec![
            CmapEntry{ value: 0, name: "zero".into(), rgb: [255,0,0], is_nodata: false },
            CmapEntry{ value: 1, name: "one".into(), rgb: [0,255,0], is_nodata: false },
            CmapEntry{ value: 2, name: "two".into(), rgb: [0,0,255], is_nodata: false },
        ])
    };
    let styles = vec![ WmsStyleConfig::Cmap( EnvPathBuf( PathBuf::from("dummy.csv"))), WmsStyleConfig::Alpha{ min: 0.0, max: 2.0 }];
    let layer = WmsLayer::new( &layer_config( &path, vec![3.0], styles), Some(&reader)).unwrap();
    assert_eq!( layer.lut_base, Some(0));
    assert!( layer.json_legend().is_some());

    let px = render( &layer, &get_map( "ndcolor=ff00ff80")); // default (first) style is cmap
    assert_eq!( px(30,50), [255,0,0,255]);   // 30 % 3 == 0
    assert_eq!( px(31,50), [0,255,0,255]);
    assert_eq!( px(32,50), [0,0,255,255]);
    assert_eq!( px(5,5), [255,0,255,128]);   // configured nodata
    assert_eq!( px(50,95), [255,0,255,128]); // declared nodata

    let px = render( &layer, &get_map( "styles=alpha&color=00ff00")); // nodata transparent by default
    assert_eq!( px(31,50), [0,255,0,128]);
    assert_eq!( px(50,95), [0,0,0,0]);
}
