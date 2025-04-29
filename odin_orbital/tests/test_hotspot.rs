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

use chrono::{Datelike,Timelike,TimeDelta};
use ron;
use std::{collections::VecDeque, fs::File};
use odin_common::{fs,cartographic::Cartographic,cartesian3::{Cartesian3,find_closest_index, dist_squared}};
use odin_orbital::{Hotspot, HotspotList, Overpass, CompletedOverpass};
use odin_orbital::firms::ViirsHotspotImporter;

// -122.15794, 37.11457
// -114.9097, 36.35744


#[test]
fn test_ground_point() {
    let input = fs::filepath_contents_as_string(&"tests/NOAA-21_VIIRS_2025-04-07_09_27.ron").unwrap();
    let op: Overpass = ron::from_str(&input).unwrap();
    let traj = &op.track;

    //gp( &traj, Cartographic::from_degrees( -122.15794, 37.11457, 0.0));
    gp( &traj, Cartographic::from_degrees( -114.9097, 36.35744, 0.0));
}

fn gp (traj: &[Cartesian3], hs: Cartographic) {
    println!("--------------------------");
    let p = Cartesian3::from(hs);
    println!("hotspot at {:.4}, {:.4}", hs.longitude_deg(), hs.latitude_deg());

    let i = find_closest_index( traj, &p);
    let j = if dist_squared( &traj[i-1], &p) > dist_squared( &traj[i+1], &p) { i+1 } else { i-1 };
    println!("closest segment: {i},{j}");

    let c0 = Cartographic::from(&traj[i-1].to_earth_radius());
    let cc = Cartographic::from(&traj[i].to_earth_radius());
    let c1 = Cartographic::from(&traj[i+1].to_earth_radius());

    println!("t[i-1] = {}  : {}", traj[i-1], traj[i-1].length());
    println!("t[i]   = {}  : {}", traj[i], traj[i].length());
    println!("t[i+1] = {}  : {}", traj[i+1], traj[i+1].length());

    println!("dist: {} : {:10.1}  :  {:.4}, {:.4}", i-1, (traj[i-1] - &p).length(), c0.longitude_deg(), c0.latitude_deg());
    println!("dist: {} : {:10.1}  :  {:.4}, {:.4}", i,   (traj[i] - &p).length(),   cc.longitude_deg(), cc.latitude_deg());
    println!("dist: {} : {:10.1}  :  {:.4}, {:.4}", i+1, (traj[i+1] - &p).length(), c1.longitude_deg(), c1.latitude_deg());

    let gp = if i > j {
        p.closest_point_on_plane( &traj[j], &traj[i]).to_earth_radius()
    } else {
        p.closest_point_on_plane( &traj[i], &traj[j]).to_earth_radius()
    };

    let cp: Cartographic = gp.into();
    println!{"closest ground point:      {:.4}, {:.4}  : {:10.1}", cp.longitude_deg(), cp.latitude_deg(),  cp.distance_to(&hs)};

    let rot = hs.bearing_to( &cp).to_degrees();
    println!("rot = {:.1} deg", rot);
    assert!( (rot + 75.9).abs() < 0.1); // expected -75.9
}

#[test]
fn test_parse () {
    let mut cops: VecDeque<CompletedOverpass<HotspotList>> = VecDeque::new();

    let op_src = fs::filepath_contents_as_string(&"tests/NOAA-21_VIIRS_2025-04-07_09_27.ron").unwrap();
    let op: Overpass = ron::from_str(&op_src).unwrap();
    cops.push_back( CompletedOverpass::new(op));

    let op_src = fs::filepath_contents_as_string(&"tests/NOAA-21_VIIRS_2025-04-07_20_48.ron").unwrap();
    let op: Overpass = ron::from_str(&op_src).unwrap();
    cops.push_back( CompletedOverpass::new(op));

    let csv_file =  File::open("tests/NOAA-21_FDDC_2025-04-07.csv").unwrap();
    let changed_ops = ViirsHotspotImporter::import_hotspots( csv_file, &mut cops).unwrap();
    println!("changed overpass indices: {:?}", changed_ops);

    for idx in changed_ops.iter() {
        if let Some(hotspot_list) =  &cops[idx].data {
            let hotspots = &hotspot_list.hotspots;
            let op = &cops[idx].overpass;
            let start = op.start;

            println!("\n---- [{}]: {} hotspots in {:04}-{:02}-{:02} {:02}:{:02} + {} min", 
                     idx, hotspots.len(), 
                     start.year(), start.month(), start.day(), start.hour(), start.minute(), 
                     (op.end - start).num_minutes());
            for h in hotspots {
                let s = serde_json::to_string(h).unwrap();
                println!("{s}");
            }

        } else {
            panic!("no data for CompletedOverpass {}", idx)
        }
    } 
}