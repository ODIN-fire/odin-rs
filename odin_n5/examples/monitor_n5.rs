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
use odin_n5::{actor::N5Actor, get_json_update_msg, get_n5_devices, live_connector::LiveN5Connector, load_config, Device, N5Config, N5DataUpdate, N5DeviceStore};
use anyhow::Result;

/* #region monitor actor *****************************************************************/

#[derive(Debug)] pub struct Snapshot(String);
#[derive(Debug)] pub struct Update(String);

define_actor_msg_set! { N5MonitorMsg = Snapshot | Update }

struct N5Monitor {}

impl_actor! { match msg for Actor<N5Monitor,N5MonitorMsg> as
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
    let hmonitor = spawn_actor!( actor_system, "monitor", N5Monitor{} )?;

    let hn5 = spawn_actor!( actor_system, "import",
        N5Actor::new( 
            LiveN5Connector::new( load_config("n5.ron")?),
            dataref_action!( 
                let hmonitor: ActorHandle<N5MonitorMsg> = hmonitor.clone() => |store: &N5DeviceStore| {
                    let json = store.get_json_snapshot_msg();
                    Ok( hmonitor.try_send_msg( Snapshot(json))? )
                }
            ),
            data_action!( 
                let hmonitor: ActorHandle<N5MonitorMsg> = hmonitor.clone() => |updates: Vec<N5DataUpdate>| {
                    let json = get_json_update_msg( &updates);
                    Ok( hmonitor.try_send_msg( Update(json))? )
                }
            )
        )
    )?;

    Ok(())
});