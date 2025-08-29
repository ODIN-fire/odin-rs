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

use std::{path::{Path,PathBuf},fs::File,io::Read};
use chrono::{DateTime,Utc,NaiveDate,NaiveTime,TimeZone};
use std::io::Cursor;
use ron;
use odin_common::{
    extract_all, extract_fields, extract_ordered, 
    fs::{filepath_contents, filepath_contents_as_string},
    u8extractor::{AsyncCsvExtractor, CsvFieldExtractor, CsvStr, MemMemFinder, SimpleU8Finder, U8Readable, read_val}
};
use odin_alertca::{create_cameras, get_cal_oes_cameras, AlertCaConfig, Camera, CameraStore};
use odin_alertca::live_connector::PropertyFinder;

#[test]
fn test_camera_update () {
    let cal_oes_cameras = get_cal_oes_cameras("resources/CalOesCameras.ron").unwrap();
    let path = Path::new("resources/all_cameras-v3.json");
    let config: AlertCaConfig = odin_build::load_config_path("configs/sf_bay_area.ron").unwrap();
    let finder = PropertyFinder::new();

    let mut store = create_cameras(&config, &cal_oes_cameras).unwrap();

    let contents = filepath_contents(&path).unwrap();
    let mut data: &[u8] = contents.as_slice();
    let mut updated: Vec<&Camera> = Vec::with_capacity(config.cameras.len());

    let t0 = std::time::SystemTime::now();
    while let Some(i0) = finder.id.find_key(data) { 
        let i1 = i0 + finder.id.len();
        if let Some((id,len)) = read_val::<&str>( data, i1) {
            if let Some(camera) = store.get( id) { // this is a camera we care about        
                let bs = &data[i1+len..];
                extract_all!{ bs ?
                    let last_frame_ts: i64 = finder.last_frame_ts,
                    let fov_lft: [f64;2] = finder.fov_lft,
                    let fov_rt: [f64;2] = finder.fov_rt,
                    let az_current: f64 = finder.az_current,
                    let tilt_current: f64 = finder.tilt_current,
                    let zoom_current: f64 = finder.zoom_current => {
                        println!("{}, {:?}, {:?}, {:?}, {}, {}, {}", 
                                  id, DateTime::from_timestamp_millis( last_frame_ts*1000).unwrap(), 
                                  fov_lft, fov_rt, az_current, tilt_current, zoom_current);
                        updated.push( camera);
                    }
                }
            }
            data = &data[i1+len..];
        } else {
            data = &data[i1..];
        }
    }
    let t1 = std::time::SystemTime::now();
    println!("update time: {} µsec", t1.duration_since(t0).unwrap().as_micros());

    assert!( updated.len() == config.cameras.len())
}