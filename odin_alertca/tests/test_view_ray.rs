/*
 * Copyright © 2026, United States Government, as represented by the Administrator of
 * the National Aeronautics and Space Administration. All rights reserved.
 *
 * The “ODIN” software is licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License. You may obtain a copy
 * of the License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software distributed under
 * the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
 * either express or implied. See the License for the specific language governing permissions
 * and limitations under the License.
 */

use std::{collections::HashMap, path::Path, sync::Arc};
use odin_common::fs::EnvPathBuf;
use odin_gdal::{Buffer, DriverManager, SpatialRef};
use odin_dem::{DemSource, DEFAULT_CURV_COEFF};
use odin_alertca::{parse_cameras, Camera, CameraAttrs};

const CAMERA_JSON: &str = r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[-121.847187,37.194245,500]},"properties":{"id":"test-cam01","name":"Test 1","fov_lft":[-121.9,37.3],"fov_rt":[-121.8,37.3],"fov_center":[-121.85,37.35],"fov":60.0,"az_current":90.0,"tilt_current":-2.0,"zoom_current":1,"ptz_updated_ts":1794251104,"last_frame_ts":1794251138}}]}"#;

fn test_camera ()->Camera {
    let id = Arc::new( "test-cam01".to_string());
    let ids = HashMap::from([ (id.to_string(), CameraAttrs { id: id.clone(), height_above_ground: 10.0, tilt_offset: -0.5 }) ]);
    let mut cameras = parse_cameras( CAMERA_JSON.as_bytes(), &ids).unwrap();
    assert_eq!( cameras.len(), 1);
    cameras.pop().unwrap()
}

/// write a flat geographic DEM of height `h` around the camera (~0.6x0.6 deg, ~20m pixels)
fn flat_dem (cam: &Camera, h: f32, path: &Path) {
    let (n, res) = (3000, 0.0002);
    let driver = DriverManager::get_driver_by_name("GTiff").unwrap();
    let mut ds = driver.create_with_band_type::<f32,_>( path, n, n, 1).unwrap();
    let (lon, lat) = (cam.position.longitude_degrees(), cam.position.latitude_degrees());
    ds.set_geo_transform( &[lon - res * n as f64 / 2.0, res, 0.0, lat + res * n as f64 / 2.0, 0.0, -res]).unwrap();
    ds.set_spatial_ref( &SpatialRef::from_epsg(4326).unwrap()).unwrap();
    ds.rasterband(1).unwrap().write( (0,0), (n,n), &mut Buffer::new( (n,n), vec![h; n*n])).unwrap();
}

#[test]
fn test_view_ray () {
    let cam = test_camera();
    println!("camera fov: {}, az: {}, tilt: {:?}", cam.fov_angle.degrees(), cam.azimut.degrees(), cam.tilt.map(|t| t.degrees()));
    let (w, h) = (1920.0, 1080.0);

    let (bearing, pitch) = cam.view_ray( 0.0, 0.0, w, h).unwrap(); // image center
    assert!( (bearing - 90.0).abs() < 1e-9);
    assert!( (pitch - -2.5).abs() < 1e-9); // includes tilt_offset

    let (bearing, _) = cam.view_ray( 1.0, 0.0, w, h).unwrap(); // right edge is at half the horizontal fov
    assert!( (bearing - 120.0).abs() < 1e-9);
    let (bearing, _) = cam.view_ray( -1.0, 0.0, w, h).unwrap();
    assert!( (bearing - 60.0).abs() < 1e-9);

    let vfov2 = (30f64.to_radians().tan() * h / w).atan().to_degrees(); // half vertical fov for 16:9 images
    let (_, pitch) = cam.view_ray( 0.0, 1.0, w, h).unwrap(); // top edge
    assert!( (pitch - (-2.5 + vfov2)).abs() < 1e-9);
}

#[tokio::test]
async fn test_locate_image_point () {
    let cam = test_camera();
    let dir = std::env::temp_dir().join( format!("odin_alertca_test_view_ray_{}", std::process::id()));
    std::fs::create_dir_all( &dir).unwrap();
    let path = dir.join("flat.tif");
    flat_dem( &cam, 200.0, &path); // note the camera reports 500m altitude - we use the DEM height + height_above_ground
    let dem = DemSource::File( EnvPathBuf( path));

    let loc = cam.locate_image_point( &dem, 0.0, 0.0, 1920.0, 1080.0, 25_000.0, 0.5, 0.0).await.unwrap().unwrap();
    let (hit, near, far) = (loc.hit.unwrap(), loc.near.unwrap(), loc.far.unwrap());
    println!("bearing {:.2}, pitch {:.2}: hit {:.1}m, near {:.1}m, far {:.1}m", loc.bearing, loc.pitch, hit.distance, near.distance, far.distance);

    let expected = |pitch: f64| 10.0 / (-pitch).to_radians().tan(); // camera is 10m above flat ground
    assert!( (hit.distance - expected(-2.5)).abs() < 0.01);
    assert!( (near.distance - expected(-3.0)).abs() < 0.01);
    assert!( (far.distance - expected(-2.0)).abs() < 0.01);
    assert!( hit.lon > cam.position.longitude_degrees()); // east of camera
    assert!( (hit.lat - cam.position.latitude_degrees()).abs() < 1e-5);

    // batch of image points: left/center/right edge on the horizontal center line, and a point above the horizon
    let pts = [(-1.0, -0.5), (0.0, -0.5), (1.0, -0.5), (0.0, 1.0)];
    let locs = cam.locate_image_points( &dem, &pts, 1920.0, 1080.0, 25_000.0, 0.5, DEFAULT_CURV_COEFF).await.unwrap();
    assert_eq!( locs.len(), 4);
    for (p,l) in pts.iter().zip( locs.iter()) {
        println!("{p:?}: bearing {:.1}, pitch {:.2}, hit {:?}", l.bearing, l.pitch, l.hit.as_ref().map( |h| h.distance));
    }
    assert!( (locs[0].bearing - 60.0).abs() < 1e-9 && (locs[2].bearing - 120.0).abs() < 1e-9);
    let d0 = locs[0].hit.as_ref().unwrap().distance;
    assert!( (d0 - locs[2].hit.as_ref().unwrap().distance).abs() < 0.01); // symmetric
    assert!( locs[3].hit.is_none()); // pitch > 0 over flat terrain

    let _ = std::fs::remove_dir_all( &dir);
}
