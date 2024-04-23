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

use anyhow::Result;
use odin_actor::prelude::*;
use odin_config::prelude::*;
use odin_sentinel::{SentinelStore,SentinelUpdate,LiveSentinelConnector,SentinelActor,EmptySnapshotAction};

use_config!();


/* #region monitor actor *****************************************************************/

#[derive(Debug)] pub struct Snapshot(String);
#[derive(Debug)] pub struct Update(String);

define_actor_msg_type! { SentinelMonitorMsg = Snapshot | Update }

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
}

/* #endregion monitor actor */


#[tokio::main]
async fn main ()->Result<()> {
    let mut actor_system = ActorSystem::with_env_tracing("main");

    let hmonitor = spawn_actor!( actor_system, "monitor", SentinelMonitor{})?;

    define_actor_action_type! { InitAction = hrcv <- (sentinels: &SentinelStore) for
        SentinelMonitorMsg => hrcv.try_send_msg( Snapshot( sentinels.to_json_pretty().unwrap()))
    }
    define_actor_action_type! { UpdateAction = hrcv <- (update: &SentinelUpdate) for
        SentinelMonitorMsg => hrcv.try_send_msg( Update( update.to_json_pretty().unwrap()))
    }

    let _hsentinel = spawn_actor!( actor_system, "sentinel", SentinelActor::new(
        LiveSentinelConnector::new( config_for!( "sentinel")?), 
        InitAction( hmonitor.clone()),
        UpdateAction( hmonitor.clone()),
        EmptySnapshotAction()
    ))?;

    actor_system.timeout_start_all(secs(2)).await?;
    actor_system.process_requests().await?;

    Ok(())
}