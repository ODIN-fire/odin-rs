/*
 * Copyright © 2024, United States Government, as represented by the Administrator of 
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

use odin_build;
use odin_actor::prelude::*;
use odin_common::datetime::secs;
use odin_sentinel::{load_config, LiveSentinelConnector, SentinelActor, SentinelStore, SentinelUpdate};


/* #region monitor actor *****************************************************************/

#[derive(Debug)] pub struct Snapshot(String);
#[derive(Debug)] pub struct Update(String);
#[derive(Debug)] pub struct Inactive(String);

define_actor_msg_set! { SentinelMonitorMsg = Snapshot | Update | Inactive }

struct SentinelMonitor {}

impl_actor! { match msg for Actor<SentinelMonitor,SentinelMonitorMsg> as
    Snapshot => cont! {
        println!("------------------------------ snapshot");
        println!("{}", msg.0);
    }
    Update => cont! { 
        println!("------------------------------ update");
        println!("{}", msg.0) 
    }
    Inactive => cont! {
        println!("------------------------------ inactive");
        println!("{}", msg.0)  
    }
}

/* #endregion monitor actor */


run_async_main!({
    odin_build::set_bin_context!();
    let mut actor_system = ActorSystem::with_env_tracing("main");

    let hmonitor = spawn_actor!( actor_system, "monitor", SentinelMonitor{})?;

    let _hsentinel = spawn_actor!( actor_system, "sentinel", SentinelActor::new(
        LiveSentinelConnector::new( load_config( "sentinel.ron")?), 
        dataref_action!( let hmonitor: ActorHandle<SentinelMonitorMsg> = hmonitor.clone() => |data:&SentinelStore| {
            let msg = Snapshot(data.to_json_pretty().unwrap());
            Ok( hmonitor.try_send_msg( msg)? )
        }),
        data_action!( let hmonitor: ActorHandle<SentinelMonitorMsg> = hmonitor.clone() => |update:SentinelUpdate| {
            let msg = Update(update.description());
            Ok( hmonitor.try_send_msg( msg)? )
        })
    ))?;

    actor_system.timeout_start_all(secs(2)).await?;
    actor_system.process_requests().await
});