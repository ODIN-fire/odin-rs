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

use odin_common::json_writer::{JsonWritable,JsonWriter};
use odin_actor::prelude::*;
use odin_alertca::{actor::AlertCaActor, live_connector::LiveAlertCaConnector, load_config, CameraStore, CameraUpdate, get_json_update_msg};
use anyhow::Result;

/* #region monitor actor *****************************************************************/

#[derive(Debug)] pub struct Snapshot(String);
#[derive(Debug)] pub struct Update(String);

define_actor_msg_set! { MonitorMsg = Snapshot | Update }

struct Monitor {}

impl_actor! { match msg for Actor<Monitor,MonitorMsg> as
    Snapshot => cont! {
        println!("------------------------------ snapshot");
        println!("{}", msg.0);
    }
    Update => cont! { 
        println!("------------------------------ update");
        println!("{}", msg.0) 
    }
}

/* endregion monitor actor */

run_actor_system!( actor_system => {
    let hmonitor = spawn_actor!( actor_system, "monitor", Monitor{} )?;

    let haca = spawn_actor!( actor_system, "import",
        AlertCaActor::new( 
            load_config("sf_bay_area.ron")?,
            LiveAlertCaConnector::new,
            dataref_action!( 
                let hmonitor: ActorHandle<MonitorMsg> = hmonitor.clone() => |store: &CameraStore| {
                    let json = store.get_json_snapshot_msg();
                    Ok( hmonitor.try_send_msg( Snapshot(json))? )
                }
            ),
            dataref_action!( 
                let hmonitor: ActorHandle<MonitorMsg> = hmonitor.clone() => |updates: &Vec<CameraUpdate>| {
                    let json = get_json_update_msg( &updates);
                    Ok( hmonitor.try_send_msg( Update(json))? )
                }
            )
        )
    )?;

    Ok(())
});