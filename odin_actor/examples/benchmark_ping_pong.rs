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

use std::time::Instant;

use odin_actor::prelude::*;
use anyhow::{anyhow,Result};
use odin_common::datetime::{millis, secs};

/// this example shows how to use PreActorHandles to break cyclic dependencies

#[derive(Debug)] pub struct Ping(u64);
#[derive(Debug)] pub struct Pong(u64);

//--- the Pinger

define_actor_msg_set! { PingerMsg = Pong }

struct Pinger<P> where P: MsgReceiver<Ping> {
    ponger: P,

    start: Instant,
    round: u64,
    max_rounds: u64,
}

impl_actor! { match msg for Actor<Pinger<P>,PingerMsg> where P: MsgReceiver<Ping> as
    //_Ping_ => cont! { msg.store_response() }
    _Start_ => cont! {
        self.start = Instant::now();
        self.ponger.try_send_msg( Ping(0));
    }
    Pong => cont! {
        self.round += 1;

        if self.round >= self.max_rounds {
            let dt = (Instant::now() - self.start).as_nanos() as u64;
            println!("{} round trips in {} ns -> {} ns/msg", self.max_rounds, dt, (dt / (2*self.max_rounds)));
        } else {
            self.ponger.try_send_msg( Ping(self.round));
        }
    }
}

//--- the Ponger

define_actor_msg_set! { PongerMsg = Ping }

struct Ponger<P> where P: MsgReceiver<Pong> {
    pinger: P
}

impl_actor! { match msg for Actor<Ponger<P>,PongerMsg> where P: MsgReceiver<Pong> as
    Ping => cont! {
        self.pinger.try_send_msg( Pong( msg.0))
    }
}

//--- the application

#[tokio::main]
async fn main ()->Result<()> {
    //console_subscriber::init();

    // for some reason the tokio main task can be very slow so we run the whole app in a spawned one
    let jh: JoinHandle<Result<()>> = tokio::spawn( async {
        let max_rounds = get_max_rounds();
        println!("-- running ping pong bench (2 actors) with {} rounds", max_rounds);
        let mut actor_system = ActorSystem::new("main");

        let pre_hpong = PreActorHandle::new( &actor_system, "ponger", 8);
        let hping = spawn_actor!( actor_system, "pinger", Pinger{ponger: pre_hpong.to_actor_handle(), start: Instant::now(), round: 0, max_rounds})?;
        let hpong = spawn_pre_actor!( actor_system, pre_hpong, Ponger{pinger: hping})?;

        actor_system.timeout_start_all(millis(20)).await?;
        actor_system.start_heartbeats(secs(5))?;

        actor_system.process_requests_for( secs(6)).await;
        Ok(())
    });

    jh.await.map_err(|e| anyhow!(e))?
}

fn get_max_rounds()->u64 {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 1 {
        1_000_000 // our default value
    } else {
        args[1].parse().expect("max round argument not an integer")
    }
}