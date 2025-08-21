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

/// example application to run single OrbitalHotspotActor

use std::sync::Arc;
use odin_actor::prelude::*;
use odin_orbital::{actor::{OrbitalHotspotActor,HotspotActorData}, firms::ViirsHotspotImporter, tle_store::SpaceTrackTleStore, *};
use odin_build::pkg_cache_dir;
use odin_common::{define_cli, geo::GeoPolygon};
use ron;

define_cli! { ARGS [about="monitor overpasses and hotspots for given satellite"] =
    region: String [help="filename of region", short, long, default_value="conus.ron"],
    sat_info: String [help="filename of OrbitalSatelliteInfo config"]
}

run_actor_system!( actor_system => {
    let cache_dir = pkg_cache_dir!();
    init_orbital_data();
    let sat_info: Arc<OrbitalSatelliteInfo> =  Arc::new( load_config( &ARGS.sat_info)?);
    let region: Arc<GeoPolygon> = Arc::new( load_config( &ARGS.region)?);

    let hmonitor = spawn_actor!( actor_system, "monitor",
        OrbitalHotspotActor::new(
            sat_info.clone(),
            region,
            SpaceTrackTleStore::new( load_config("spacetrack.ron")?, sat_info.clone(), Some(cache_dir.clone())),
            ViirsHotspotImporter::new( load_config("firms.ron")?, sat_info.clone(), cache_dir.clone()),
            dataref_action!( => |data: &HotspotActorData| {
                println!("-- actor initialized with {} past and {} future overpasses", data.completed.len(), data.upcoming.len());
                for co in &data.completed { println!("past:     {}", co.overpass) }
                for o in &data.upcoming   { println!("upcoming: {}", o) }
                Ok(())
            }),
            data_action!( => |overpass: Vec<&Overpass>| {
                for over in overpass {
                    println!("{}", over);
                }
                Ok(())
            }),
            data_action!( => |hs: Vec<&HotspotList>| {
                println!("-- got data with {} hotspots starting at {:?}", hs.len(),
                    hs.first().map(|h| h.start));
                //let s = ron::ser::to_string_pretty( hs, ron::ser::PrettyConfig::default().compact_structs(true))?;
                //println!("{s}");
                Ok(())
            })
        )       
    )?;

    Ok(())
});