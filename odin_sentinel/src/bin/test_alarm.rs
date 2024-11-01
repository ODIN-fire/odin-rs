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

use std::path::Path;
use tokio;
use chrono::Utc;
use odin_common::{angle::{LatAngle, LonAngle}, define_cli, geo::DatedGeoPos};
use odin_sentinel::{load_config, Alarm, AlarmMessenger, EvidenceInfo, 
    ConsoleAlarmMessenger, SmtpAlarmMessenger, SignalCmdAlarmMessenger, SlackAlarmMessenger, SentinelFile
};
use anyhow::Result;
 
define_cli! { ARGS [about="Delphire Sentinel Slack alarm test"] = 
    slack: bool                   [help="enable slack messenger", long],
    smtp: bool                    [help="enable smtp messenger", long],
    signal_cli: bool              [help="enable signal-cli messenger (requires signal-cli installation)", long],

    device: String                [help="device_id", default_value="test-device",short,long],
    alarm_type: String            [help="alarm type", default_value="smoke", short,long],
    confidence: f64               [help="confidence [0.0 .. 1.0]", default_value="0.70", short,long],
    img: Option<String>           [help="optional pathname of image to attach", short, long],
    text: Option<String>          [help="optional alarm notification text", short, long]
}

/// test application for alarm messengers - this sends artificial alarms to the messenger types
/// specified as command line arguments (console is always enabled)
/// Note this uses the same config files from the ODIN installation as the sentinel_alarm server
#[tokio::main]
async fn main()->Result<()> {
    let device_id = ARGS.device.clone();
    let description = if let Some(descr) = &ARGS.text { descr.clone() } else { "test alarm".into() };
    let time_recorded = Utc::now();
    let pos = Some( DatedGeoPos::new(LatAngle::from_degrees(37.1668), LonAngle::from_degrees(-121.9633), 560.0, time_recorded));
    let alarm_type = ARGS.alarm_type.clone();
    let confidence = ARGS.confidence;
    let mut evidence_info: Vec<EvidenceInfo> = Vec::new();

    if let Some(img) = &ARGS.img {
        let pathname = Path::new(&img).to_path_buf();
        if !pathname.is_file() { panic!("image file does not exist: {img}") }

        let ei = EvidenceInfo { 
            sensor_no: 0, 
            description: "visual".to_string(), 
            img: Some(SentinelFile { record_id: "image".to_string(), pathname })
        };
        evidence_info.push(ei);
    }

    let alarm = Alarm { device_id, description, time_recorded, pos, alarm_type, confidence, evidence_info };

    let messengers = create_messengers()?;
    
    for m in &messengers {
        let res = m.send_alarm(&alarm).await?;
        println!("result = {res:?}");
    }

    Ok(())
}

fn create_messengers()->Result<Vec<Box<dyn AlarmMessenger>>> {
    let mut messengers: Vec<Box<dyn AlarmMessenger>> = Vec::new();

    messengers.push( Box::new(ConsoleAlarmMessenger{})); // always enabled

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