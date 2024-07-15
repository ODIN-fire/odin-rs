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
use anyhow::{anyhow,Result};

//--- the actor implementation

#[derive(Debug)]
pub struct Greet (pub &'static str);
//... define any other message struct our actor would process here

define_actor_msg_set! { pub GreeterMsg = Greet }

pub struct Greeter; // look ma - no fields (those would be the actor state)

impl_actor! { match msg for Actor<Greeter,GreeterMsg> as
    Greet => term! { println!("hello {}!", msg.0); }
}

//--- the application using the actor

#[tokio::main]
async fn main() ->Result<()> {
    let mut actor_system = ActorSystem::with_env_tracing("main");

    let actor_handle = spawn_actor!( actor_system, "greeter", Greeter{})?;

    actor_handle.send_msg( Greet("world")).await?;

    actor_system.process_requests().await?;

    Ok(())
}
