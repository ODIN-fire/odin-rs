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

//! tests for WMS request parsing (the subset required by Cesium.WebMapServiceImageryProvider)

use std::collections::HashMap;
use odin_gdal::{geods::{GeoDsImgType, GeoDsSrs}, geods_srv::{WmsRequest, parse_wms_request}};

fn query (s: &str)->HashMap<String,String> {
    s.split('&').filter_map( |kv| kv.split_once('=')).map( |(k,v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn test_cesium_v111 () {
    // what Cesium.WebMapServiceImageryProvider sends by default (version 1.1.1 uses SRS and lon,lat bbox order)
    let q = query("service=WMS&version=1.1.1&request=GetMap&styles=&format=image/png&transparent=true&layers=LF2025_EVT&srs=EPSG:4326&bbox=-122.5,37.0,-122.0,37.5&width=256&height=256&color=ff8000");
    let Ok(WmsRequest::GetMap(gm)) = parse_wms_request( &q) else { panic!("not a GetMap request") };

    assert_eq!( gm.layers, vec!["LF2025_EVT".to_string()]);
    assert!( matches!( gm.srs, GeoDsSrs::WGS84));
    assert!( matches!( gm.img_type, GeoDsImgType::PNG));
    assert!( gm.transparent);
    assert_eq!( (gm.bbox.west, gm.bbox.south, gm.bbox.east, gm.bbox.north), (-122.5, 37.0, -122.0, 37.5));
    assert_eq!( (gm.width, gm.height), (256, 256));
    assert_eq!( gm.params.get("color").map( |s| s.as_str()), Some("ff8000")); // vendor specific parameter
}

#[test]
fn test_v130_axis_order () {
    // WMS 1.3.0 uses lat,lon axis order for EPSG:4326 (but not for CRS:84). Parameter names are case insensitive
    let q = query("SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&FORMAT=image/png&LAYERS=a&CRS=EPSG:4326&BBOX=37.0,-122.5,37.5,-122.0&WIDTH=256&HEIGHT=128");
    let Ok(WmsRequest::GetMap(gm)) = parse_wms_request( &q) else { panic!("not a GetMap request") };
    assert_eq!( (gm.bbox.west, gm.bbox.south, gm.bbox.east, gm.bbox.north), (-122.5, 37.0, -122.0, 37.5));
    assert_eq!( (gm.width, gm.height), (256, 128));

    let q = query("service=WMS&version=1.3.0&request=GetMap&format=image/png&layers=a&crs=CRS:84&bbox=-122.5,37.0,-122.0,37.5&width=256&height=256");
    let Ok(WmsRequest::GetMap(gm)) = parse_wms_request( &q) else { panic!("not a GetMap request") };
    assert_eq!( (gm.bbox.west, gm.bbox.south, gm.bbox.east, gm.bbox.north), (-122.5, 37.0, -122.0, 37.5));
}

#[test]
fn test_get_capabilities () {
    let q = query("service=WMS&request=GetCapabilities");
    assert!( matches!( parse_wms_request( &q), Ok(WmsRequest::GetCapabilities)));
}

#[test]
fn test_errors () {
    let e = parse_wms_request( &query("service=WMS&request=GetLegendGraphic")).unwrap_err();
    assert_eq!( e.code, Some("OperationNotSupported"));

    let e = parse_wms_request( &query("service=WMS&version=1.1.1&request=GetMap&format=image/png&layers=a&bbox=0,0,1,1&width=256&height=256")).unwrap_err();
    assert_eq!( e.code, Some("MissingParameterValue")); // no SRS

    let e = parse_wms_request( &query("service=WMS&version=1.1.1&request=GetMap&format=image/png&layers=a&srs=EPSG:4326&bbox=1,0,0,1&width=256&height=256")).unwrap_err();
    assert_eq!( e.code, Some("InvalidParameterValue")); // west > east

    let e = parse_wms_request( &query("service=WMS&version=1.1.1&request=GetMap&format=image/gif&layers=a&srs=EPSG:4326&bbox=0,0,1,1&width=256&height=256")).unwrap_err();
    assert_eq!( e.code, Some("InvalidFormat"));
}

#[test]
fn test_get_feature_info () {
    // what Cesium.WebMapServiceImageryProvider sends for pickFeatures (1.1.1 uses X,Y pixel coordinates)
    let q = query("service=WMS&version=1.1.1&request=GetFeatureInfo&styles=&format=image/png&transparent=true&layers=a&query_layers=a&info_format=application/json&srs=EPSG:4326&bbox=-122.0,37.0,-121.0,38.0&width=256&height=256&x=0&y=255");
    let Ok(WmsRequest::GetFeatureInfo(gfi)) = parse_wms_request( &q) else { panic!("not a GetFeatureInfo request") };
    assert_eq!( gfi.query_layers, vec!["a".to_string()]);
    assert_eq!( (gfi.i, gfi.j), (0, 255));

    let (x,y) = gfi.position(); // pixel center
    assert!( (x - (-122.0 + 0.5/256.0)).abs() < 1e-9);
    assert!( (y - (37.0 + 0.5/256.0)).abs() < 1e-9);

    // 1.3.0 uses I,J and has optional FORMAT/LAYERS
    let q = query("service=WMS&version=1.3.0&request=GetFeatureInfo&query_layers=a&crs=CRS:84&bbox=-122.0,37.0,-121.0,38.0&width=256&height=256&i=128&j=128");
    let Ok(WmsRequest::GetFeatureInfo(gfi)) = parse_wms_request( &q) else { panic!("not a GetFeatureInfo request") };
    assert_eq!( gfi.info_format, "application/json");

    let e = parse_wms_request( &query("service=WMS&version=1.1.1&request=GetFeatureInfo&query_layers=a&srs=EPSG:4326&bbox=0,0,1,1&width=256&height=256&x=256&y=0")).unwrap_err();
    assert_eq!( e.code, Some("InvalidPoint"));

    let e = parse_wms_request( &query("service=WMS&version=1.1.1&request=GetFeatureInfo&query_layers=a&info_format=text/html&srs=EPSG:4326&bbox=0,0,1,1&width=256&height=256&x=0&y=0")).unwrap_err();
    assert_eq!( e.code, Some("InvalidFormat"));
}
