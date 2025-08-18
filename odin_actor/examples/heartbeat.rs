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

use std::{any, fmt::Debug, time::Duration};
use odin_actor::{console_ui::ConsoleUI, prelude::*};
use odin_common::datetime::millis;
use odin_macro::fn_mut;
use tokio::task::JoinHandle;
use anyhow::{anyhow,Result};

#[cfg(feature="tui")]
use odin_actor::tui;
use odin_common::datetime::secs;
//--- Actor1

#[derive(Debug)] struct MsgA(usize);
#[derive(Debug)] struct MsgB(usize);

define_actor_msg_set! { Actor1Msg = MsgA | MsgB }

struct Actor1State {
    n_a: usize,
    n_b: usize
}

impl_actor! { match msg for Actor<Actor1State,Actor1Msg> as 
    MsgA => cont!{ self.n_a += 1 }
    MsgB => cont!{ self.n_b += 1 }
}

//--- Actor2

#[derive(Debug)] struct MsgC(usize);
define_actor_msg_set! { Actor2Msg = MsgC }

struct Actor2State {
    n: usize,
    a1: ActorHandle<Actor1Msg>
}

impl_actor! { match msg for Actor<Actor2State,Actor2Msg> as 
    _Start_ => cont!{ 
        self.start_repeat_timer(1, millis(100), false) 
    }
    _Timer_ => cont!{
        self.n += 1;
        self.a1.try_send_msg( MsgB(self.n));
    }
    MsgC => cont!{
        self.a1.try_send_msg( MsgA(msg.0))
    }
}

//--- Actor3

define_actor_msg_set! { Actor3Msg }

struct Actor3State {
    a1: ActorHandle<Actor1Msg>,
    a2: ActorHandle<Actor2Msg>,
}

impl_actor! { match msg for Actor<Actor3State,Actor3Msg> as 
    _Start_ => cont! {
        if let Ok(mut scheduler) = self.get_scheduler() {
            scheduler.schedule_repeated(Duration::ZERO, millis(60), fn_mut!{
                (a1 = self.a1.clone(), a2 = self.a2.clone(), mut n = 0) => |_ctx| {
                    a1.try_send_msg(MsgA(n));
                    a2.try_send_msg(MsgC(n));
                    n += 1;
                    if n % 100 == 0 { 
                        // println!("{n}");
                        //print!("\r\x1b[32;1m  \x1b[37m {}\x1b[0m", n) 
                    }
                }  
            });
        }
    }
}

//--- application

//--- the application using the actor

//#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
#[tokio::main]
async fn main() ->Result<()> {
    //console_subscriber::init();
    //tracing_subscriber::fmt::init();

    // for some reason the tokio main task can be very slow so we run the whole app in a spawned one
    let jh: JoinHandle<Result<()>> = tokio::spawn( async {
        let mut actor_system = ActorSystem::new("main");

        // use the ratatui UI if example was built with the 'tui' feature ...
        #[cfg(feature="tui")]
        actor_system.set_ui(tui::create_tui(actor_system.clone_handle()).await?);

        //... otherwise use plain console output
        #[cfg(not(feature="tui"))]
        actor_system.set_ui( ConsoleUI::new_boxed( actor_system.clone_handle()));


        let a1 = spawn_actor!( actor_system, "actor1", Actor1State{n_a: 0, n_b: 0})?;
        let a2 = spawn_actor!( actor_system, "actor2", Actor2State{n: 0, a1: a1.clone()})?;
        let a3 = spawn_actor!( actor_system, "actor3", Actor3State{a1,a2})?;

        actor_system.start_all().await?;
        actor_system.start_heartbeats(secs(5))?;
        actor_system.process_requests().await?;

        Ok(())
    });
    jh.await.map_err(|e| anyhow!(e))?
}
