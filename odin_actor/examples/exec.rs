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

/// example of how to execute Fn closures in own actor task without explicit message types
/// (using the implicit _Exec_ system message)

use odin_actor::prelude::*;
use std::fmt::Debug;
use anyhow::{anyhow,Result};
use odin_common::datetime::{millis, secs};

#[derive(Debug)] struct Greet (&'static str);

//... define any other message struct our actor would process here
define_actor_msg_set! { GreeterMsg = Greet }

struct Greeter; // look ma - no fields


impl_actor! { match msg for Actor<Greeter,GreeterMsg> as
    Greet => cont! { 
        println!("got greeting: hello {}!", msg.0); 

        if msg.0 != "me" {
            let myself = self.hself.clone();
            self.exec( move|| { // this is turned into a generic _Exec_ system message sent to ourself
                println!("now trying to be nice to myself..");
                myself.try_send_msg(Greet("me")); 
            });
        }
    }
}

pub struct Blah;

#[tokio::main]
async fn main() ->Result<()> {
    let mut actor_system = ActorSystem::new("main");

    let actor_handle = spawn_actor!( actor_system, "greeter", Greeter{})?;

    actor_handle.send_msg( Greet("world")).await?;

    sleep( secs(1)).await;
    actor_system.terminate_and_wait( millis(20)).await?;
    Ok(())
}