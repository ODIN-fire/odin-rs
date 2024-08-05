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

#![allow(unused)]

use anyhow::Result;
use odin_build::define_load_config;
use odin_actor::prelude::*;
use odin_common::{define_cli,check_cli, admin, heap};
use odin_sentinel::{
    AlarmMessenger, ConsoleAlarmMessenger, LiveSentinelConnector, SentinelActor, SentinelAlarmMonitor, SentinelAlarmMonitorMsg, 
    SentinelUpdate, SlackAlarmMessenger, SmtpAlarmMessenger, SignalCmdAlarmMessenger,
    load_config,
};

//heap::use_dhat!{} // for debugging - only has an effect if we build with `--features dhat` 

// use odin_sentinel::SignalRpcAlarmMessenger; // don't use for now since it does not support notify-self
 
define_cli! { ARGS [about="Delphire Sentinel Alarm Server"] = 
    slack: bool       [help="enable slack messenger", long],
    smtp: bool        [help="enable smtp messenger", long],
    signal_cli: bool  [help="enable signal-cli messenger (requires signal-cli installation)", long],
    console: bool     [help="enable console messenger",long]
}

#[tokio::main]
async fn main ()->Result<()> {
    odin_build::set_bin_context!();
    admin::monitor_executable!();

    //heap::init_dhat!(); // for debugging - only has an effect if we build with `--features dhat`

    check_cli!(ARGS);
    let mut actor_system = ActorSystem::with_env_tracing("main");
    actor_system.request_termination_on_ctrlc(); // don't just exit without notification

    let hsentinel = PreActorHandle::new( &actor_system, "sentinel", 8); 

    let hmonitor = spawn_actor!( actor_system, "monitor", SentinelAlarmMonitor::new(
        load_config("sentinel_alarm.ron")?,
        hsentinel.as_actor_handle(),
        create_messengers()?
    ))?;

    let hsentinel = spawn_pre_actor!( actor_system, hsentinel, SentinelActor::new(
        LiveSentinelConnector::new( load_config( "sentinel.ron")?), 
        no_dataref_action(),
        data_action!( hmonitor: ActorHandle<SentinelAlarmMonitorMsg> => |data:SentinelUpdate| hmonitor.try_send_msg(data)),
    ))?;

    actor_system.timeout_start_all(millis(20)).await?;
    actor_system.process_requests().await?;

    Ok(())
}

fn create_messengers()->Result<Vec<Box<dyn AlarmMessenger>>> {
    let mut messengers: Vec<Box<dyn AlarmMessenger>> = Vec::new();

    if ARGS.console {
        messengers.push( Box::new(ConsoleAlarmMessenger{}));
    }

    if ARGS.slack {
        messengers.push( Box::new( SlackAlarmMessenger::new( load_config("slack_alarm.ron")?)))
    }
    if ARGS.smtp { 
        messengers.push( Box::new( SmtpAlarmMessenger::new( load_config("smtp")?))) 
    }
    if ARGS.signal_cli { 
        messengers.push( Box::new( SignalCmdAlarmMessenger::new( load_config("signal_cmd")?))) 
    }

    Ok(messengers)
}