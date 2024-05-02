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
 use odin_sentinel::{
    SentinelUpdate,LiveSentinelConnector,SentinelActor,
    SentinelAlarmMonitor,ConsoleMessenger
};
 
 use_config!();

 #[tokio::main]
async fn main ()->Result<()> {
    let mut actor_system = ActorSystem::with_env_tracing("main");

    let hsentinel = PreActorHandle::new( &actor_system, "sentinel", 8); 

    let hmonitor = spawn_actor!( actor_system, "monitor", SentinelAlarmMonitor::new(
        config_for!("sentinel-alarm")?,
        hsentinel.as_actor_handle(),
        ConsoleMessenger{}
    ))?;

    let _hsentinel = spawn_pre_actor!( actor_system, hsentinel, SentinelActor::new(
        LiveSentinelConnector::new( config_for!( "sentinel")?), 
        NoDataRefAction::new(),
        data_action!( hmonitor as MsgReceiver<SentinelUpdate> => |data:SentinelUpdate| hmonitor.try_send_msg(data)),
        NoLabeledDataRefAction::new()
    ))?;

    actor_system.timeout_start_all(millis(20)).await?;
    actor_system.process_requests().await?;

    Ok(())
}