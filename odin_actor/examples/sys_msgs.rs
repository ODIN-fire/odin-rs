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

use odin_actor::prelude::*;
use odin_common::datetime::secs;
use std::time::Duration;
use anyhow::{anyhow,Result};

struct Ticker {
    count: u64,

    interval: Duration,
    timer: Option<AbortHandle>
}
impl Ticker {
    fn new (interval: Duration)->Self { Ticker { count: 0, interval, timer: None } }
}

define_actor_msg_set! { TickerMsg }

impl_actor! { match msg for Actor<Ticker,TickerMsg> as
    _Start_ => cont! { 
        if let Ok(timer) = self.start_repeat_timer( 1, self.interval, false) {
            self.timer = Some(timer);
            println!("started timer in '{}' for 10 ticks", self.hself.id());
        }
    }
    _Timer_ => { 
        self.count += 1;
        println!("tick {}", self.count);
        if self.count > 10 { ReceiveAction::RequestTermination } else { ReceiveAction::Continue }
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
    actor_system.request_termination_on_ctrlc(); // don't just exit the process - shut down gracefully

    //let actor_handle = actor_system.spawn_act( actor_system.new_actor("ticker", Ticker::new( secs(1)), 8))?;
    let actor_handle = spawn_actor!( actor_system, "ticker", Ticker::new( secs(1)));

    actor_system.start_all().await?; // sends out _Start_ messages
    actor_system.process_requests().await?;

    println!("actor system terminated."); // would not show without setting termination handler
    Ok(())
}