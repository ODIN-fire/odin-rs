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
    load_config, AlarmMessenger, ConsoleAlarmMessenger, LiveSentinelConnector, SentinelActor, SentinelAlarmMonitor, SentinelAlarmMonitorConfig, SentinelAlarmMonitorMsg, SentinelInactiveAlert, SentinelUpdate, SignalCmdAlarmMessenger, SlackAlarmMessenger, SmtpAlarmMessenger
};

#[cfg(feature="dhat")] heap::use_dhat!{} 
 
define_cli! { ARGS [about="Delphire Sentinel Alarm Server"] = 
    fire_prob: f64    [help="override fire probability threshold [0.0..1.0]", long, default_value="-1.0"],
    smoke_prob: f64   [help="override smoke probability threshold [0.0..1.0]", long, default_value="-1.0"],

    slack: bool       [help="enable slack messenger", long],
    smtp: bool        [help="enable smtp messenger", long],
    signal_cli: bool  [help="enable signal-cli messenger (requires signal-cli installation)", long],
    console: bool     [help="enable console messenger",long]
}

#[tokio::main]
async fn main ()->Result<()> {
    odin_build::set_bin_context!();
    admin::monitor_executable!();

    #[cfg(feature="dhat")] heap::init_dhat!();

    check_cli!(ARGS);
    let mut actor_system = ActorSystem::with_env_tracing("main");
    actor_system.request_termination_on_ctrlc(); // don't just exit without notification

    let hsentinel = PreActorHandle::new( &actor_system, "sentinel", 8); 
    
    let hmonitor = spawn_actor!( actor_system, "monitor", SentinelAlarmMonitor::new(
        load_alarm_conf()?,
        load_config("sentinel_info.ron")?,
        hsentinel.to_actor_handle(),
        create_messengers()?
    ))?;

    let hsentinel = spawn_pre_actor!( actor_system, hsentinel, SentinelActor::new(
        LiveSentinelConnector::new( load_config( "sentinel.ron")?), 
        no_dataref_action(),
        data_action!( let hmonitor: ActorHandle<SentinelAlarmMonitorMsg> = hmonitor.clone() => |data:SentinelUpdate| Ok( hmonitor.try_send_msg(data)? )),
        data_action!( let hmonitor: ActorHandle<SentinelAlarmMonitorMsg> = hmonitor => |data:SentinelInactiveAlert| Ok( hmonitor.try_send_msg(data)? )),
    ))?;

    actor_system.timeout_start_all(millis(20)).await?;
    actor_system.process_requests().await?;

    Ok(())
}

fn load_alarm_conf()->Result<SentinelAlarmMonitorConfig> {
    let mut alarm_conf: SentinelAlarmMonitorConfig = load_config("sentinel_alarm.ron")?;
    if ARGS.fire_prob >= 0.0 { alarm_conf.fire_prob = ARGS.fire_prob }
    if ARGS.smoke_prob >= 0.0 { alarm_conf.smoke_prob = ARGS.smoke_prob }

    Ok(alarm_conf)
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
        messengers.push( Box::new( SmtpAlarmMessenger::new( load_config("smtp.ron")?))) 
    }
    if ARGS.signal_cli { 
        messengers.push( Box::new( SignalCmdAlarmMessenger::new( load_config("signal_cmd.ron")?))) 
    }

    Ok(messengers)
}