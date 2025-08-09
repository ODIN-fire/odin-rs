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

use chrono::{DateTime,Utc};
use odin_actor::prelude::*;
use odin_common::json_writer::JsonWriter;
use odin_adsb::{load_config, Aircraft, AircraftStore, actor::AdsbActor, sbs::SbsConnector};
use anyhow::Result;

run_actor_system!( actor_system => {
    let hadsb = spawn_actor!( actor_system, "adsb",
        AdsbActor::<SbsConnector,_>::new(
            load_config("adsb.ron")?, 
            dataref_mut_action!(  let mut w: JsonWriter = JsonWriter::with_capacity(4096) => |store: &AircraftStore| {
                println!("------------------ {}", store.timestamp());

                store.write_json_update_to(w);
                //store.write_json_snapshot_to(w);
                //println!("{}", w.as_str());

                
                for e in store.aircraft() {
                    let ac = e.value();
                    if let Some(p) = ac.last_position() {
                        println!("{}", ac);
                    }
                }
                

                Ok(())
            })
        )
    )?;

    Ok(())
});