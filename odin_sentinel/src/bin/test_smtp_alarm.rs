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

use std::{fs,path::Path};
use tokio;
use odin_common::define_cli;
use odin_sentinel::{Alarm, EvidenceInfo, AlarmMessenger, SmtpConfig, SmtpAlarmMessenger, SentinelFile};
use anyhow::Result;

define_cli! { ARGS [about="Delphire Sentinel SMTP alarm test"] = 
    img: Option<String>    [help="optional pathname of image to attach", short, long],
    config: String         [help="pathname of SMTP alarm config to test"]
}

#[tokio::main]
async fn main()->Result<()> {
    let config: SmtpConfig = ron::from_str(fs::read_to_string(&ARGS.config)?.as_str())?;
    
    let alarm = if let Some(img) = &ARGS.img {
        let pathname = Path::new(&img).to_path_buf();
        if !pathname.is_file() { panic!("image file does not exist: {img}") }
        Alarm { 
            description: "test alarm".to_string(), 
            evidence_info: vec!( 
                EvidenceInfo { 
                    description: "visual".to_string(), 
                    img: Some(SentinelFile { record_id: "image".to_string(), pathname })
                }
            ) 
        }
    } else {
        Alarm { 
            description: "test alarm".to_string(), 
            evidence_info: Vec::new() 
        }
    };

    let messenger = SmtpAlarmMessenger::new(config);
    
    let res = messenger.send_alarm(&alarm).await?;
    println!("result = {res:?}");

    Ok(())
}
