/*
 * Copyright © 2025, United States Government, as represented by the Administrator of
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

#![allow(unused)]

use std::path::Path;
use chrono::DateTime;
use odin_common::fs::filepath_contents;
use odin_alertca::{camera_ids, parse_cameras, AlertCaConfig, Camera, CameraStore};

/// run with "cargo test --test test_all_cameras test_camera_update -- --nocapture"
#[test]
fn test_camera_update () {
    let path = Path::new("resources/all_cameras.json");
    let config: AlertCaConfig = odin_build::load_config_path("configs/sf_bay_area.ron").unwrap();
    let ids = camera_ids( &config);

    let contents = filepath_contents(&path).unwrap();

    let t0 = std::time::SystemTime::now();
    let cameras = parse_cameras( &contents, &ids).unwrap();
    let t1 = std::time::SystemTime::now();

    for c in &cameras {
        println!("{:25} {:28} {:10.5} {:9.5} {:6.0}m  {:?}  fov: {:3.0}° {:6.0}m  az: {:3.0}  tilt: {:?}  zoom: {:?}",
                 c.id, c.name, c.position.longitude_degrees(), c.position.latitude_degrees(), c.position.altitude_meters(),
                 DateTime::from_timestamp_millis( c.last_update.millis()).unwrap(),
                 c.fov_angle.degrees(), c.fov_dist.get::<uom::si::length::meter>(), c.azimut.degrees(),
                 c.tilt.map(|t| t.degrees()), c.zoom);
    }
    println!("parse time: {} µsec", t1.duration_since(t0).unwrap().as_micros());
    println!("parsed {} cameras for {} configured ids", cameras.len(), config.cameras.len());

    assert_eq!( cameras.len(), config.cameras.len());
    for c in &cameras {
        assert!( config.cameras.iter().any( |cc| cc.id.as_str() == c.id.as_str()));
        assert!( !c.name.is_empty());
        assert!( c.position.altitude_meters() > 0.0);
    }

    // check values of a PTZ camera and a fixed camera (az/tilt/zoom are null in the GeoJSON)
    let almaden1 = cameras.iter().find( |c| c.id.as_str() == "almaden-cam01").unwrap();
    assert_eq!( almaden1.name, "Almaden 1");
    assert!( (almaden1.position.longitude_degrees() - -121.847187).abs() < 1e-6);
    assert!( (almaden1.position.latitude_degrees() - 37.194245).abs() < 1e-6);
    assert!( almaden1.tilt.is_some() && almaden1.zoom.is_some());

    let almaden2 = cameras.iter().find( |c| c.id.as_str() == "almaden-cam02").unwrap();
    assert!( almaden2.tilt.is_none() && almaden2.zoom.is_none());
    assert!( almaden2.fov_angle.degrees() > 0.0);

    // store initialization and JSON snapshot
    let mut store = CameraStore::new( config.max_history);
    store.update_all( cameras);
    assert_eq!( store.len(), config.cameras.len());

    let msg = store.get_json_snapshot_msg();
    assert!( msg.contains("\"name\":\"Almaden 1\""));
}


#[test]
fn test_camera_config () {
    // cameras can be specified as plain ids or as structs with optional height_above_ground and tilt_offset
    let config: AlertCaConfig = ron::from_str( r#"AlertCaConfig(
        cameras: [
            "almaden-cam01",
            (id: "almaden-cam02", height_above_ground: 25.0),
            (id: "lomaprieta-cam01", tilt_offset: -0.5),
            (id: "mtchual-cam01", height_above_ground: 15.0, tilt_offset: 0.75),
        ],
        default_height_above_ground: 8.0,
        base_url: "https://cameras.alertcalifornia.org/public-camera-data",
        update_interval: Duration(secs:180,nanos:0),
        max_history: 10,
        max_age: Duration(secs:3600,nanos:0),
    )"#).unwrap();

    assert_eq!( config.cameras.len(), 4);
    assert_eq!( config.default_height_above_ground, 8.0);
    let cc: Vec<_> = config.cameras.iter().map( |cc| config.camera_attrs( cc)).collect();
    assert_eq!( (cc[0].id.as_str(), cc[0].height_above_ground, cc[0].tilt_offset), ("almaden-cam01", 8.0, 0.0)); // config default
    assert_eq!( (cc[1].id.as_str(), cc[1].height_above_ground, cc[1].tilt_offset), ("almaden-cam02", 25.0, 0.0));
    assert_eq!( (cc[2].id.as_str(), cc[2].height_above_ground, cc[2].tilt_offset), ("lomaprieta-cam01", 8.0, -0.5));
    assert_eq!( (cc[3].id.as_str(), cc[3].height_above_ground, cc[3].tilt_offset), ("mtchual-cam01", 15.0, 0.75));

    // the configured values end up in the parsed cameras and their JSON
    let contents = filepath_contents( Path::new("resources/all_cameras.json")).unwrap();
    let cameras = parse_cameras( &contents, &camera_ids( &config)).unwrap();
    let almaden2 = cameras.iter().find( |c| c.id.as_str() == "almaden-cam02").unwrap();
    assert_eq!( almaden2.height_above_ground, 25.0);

    let mut store = CameraStore::new( config.max_history);
    store.update_all( cameras);
    let msg = store.get_json_snapshot_msg();
    assert!( msg.contains("\"heightAboveGround\":25.0"));
    assert!( msg.contains("\"tiltOffset\":-0.50"));
}
