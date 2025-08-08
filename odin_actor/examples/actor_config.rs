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

// example for actor configuration
// has to be run from odin_actor/ dir

use odin_build;
use odin_actor::prelude::*;

use std::{time::Duration,default::Default};
use anyhow::{anyhow,Result};
use ron::de;
use serde::Deserialize;
use odin_common::datetime::{millis, secs};

#[derive(Deserialize)]
struct TickerConfig {
    interval_sec: u64,
}
impl Default for TickerConfig {
    fn default()->Self { TickerConfig { interval_sec: 1 } }
}

define_actor_msg_set! { TickerMsg }

struct Ticker {
    config: TickerConfig,

    count: u64,
    timer: Option<AbortHandle>
}
impl Ticker {
    fn new (config: TickerConfig)->Self { 
        Ticker { config, count: 0, timer: None }
    }
}

impl_actor! { match msg for Actor <Ticker,TickerMsg>  as 
    _Start_ => cont! { 
        if let Ok(timer) = self.start_repeat_timer( 1, secs(self.config.interval_sec), false) {
            self.timer = Some(timer);
            println!("started timer")
        }
    }
    _Timer_ => cont! { 
        self.count += 1;
        println!("tick {}", self.count);
    }
    _Terminate_ => stop! {
        if let Some(timer) = &self.timer { 
            timer.abort();
            self.timer = None;
        }
        println!("terminated timer");
    }
}


#[tokio::main]
async fn main() ->Result<()> {
    let mut actor_system = ActorSystem::new("main");

    // note this assumes the app is running in the parent workspace dir 
    // (normally this would be loaded through the load_config(..) function of the crate)
    let ticker_config: TickerConfig = odin_build::load_config_path("examples/config/ticker.ron")?;
    let ticker_handle = spawn_actor!( actor_system, "ticker", Ticker::new( ticker_config))?;

    actor_system.timeout_start_all(millis(20)).await?; // sends out _Start_ messages
    sleep( secs(5)).await;
    actor_system.terminate_and_wait( millis(20)).await?;  // sends out _Terminate_ messages and waits for actor completion

    Ok(())
}