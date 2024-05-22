/*
 * Copyright (c) 2024, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The ODIN - Open Data Integration Framework is licensed under the
 * Apache License, Version 2.0 (the "License"); you may not use this file
 * except in compliance with the License. You may obtain a copy of the
 * License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

#![allow(unused)]

use tokio;
use anyhow::Result;
use odin_actor::prelude::*;
use odin_goesr::actor::GoesRImportActor;
use odin_goesr::{live_importer::LiveGoesRDataImporter, GoesRHotSpots};
use odin_config::prelude::*;

use_config!();

#[derive(Debug)] pub struct Update(String);

define_actor_msg_set! { GoesRMonitorMsg = Update }
struct GoesRMonitor {}

impl_actor! { match msg for Actor<GoesRMonitor,GoesRMonitorMsg> as
    Update => cont! { 
        println!("------------------------------ update");
        println!("{}", msg.0) 
    }
}


#[tokio::main]
async fn main() -> Result<()>{
    let mut actor_system = ActorSystem::with_env_tracing("main");

    let hmonitor = spawn_actor!( actor_system, "monitor", GoesRMonitor{})?;

    let _actor_handle = spawn_actor!( actor_system, "goesr",  GoesRImportActor::new(
        config_for!( "goesr")?, 
        LiveGoesRDataImporter::new( config_for!( "goes_18_aws")?),
        data_action!( hmonitor.clone(): ActorHandle<GoesRMonitorMsg> => |data:Vec<GoesRHotSpots>| {
            for hs in data.into_iter(){
                let msg = Update(hs.to_json_pretty().unwrap());
                hmonitor.try_send_msg(msg);
            }
            action_ok()
        }),
        data_action!( hmonitor: ActorHandle<GoesRMonitorMsg> => |data:GoesRHotSpots| {
            let msg = Update(data.to_json_pretty().unwrap());
            hmonitor.try_send_msg( msg)
        }),
    ))?;


    actor_system.start_all().await?;
    actor_system.process_requests().await;

    Ok(())
}