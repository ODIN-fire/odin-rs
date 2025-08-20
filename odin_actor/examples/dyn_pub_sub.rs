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

use odin_actor::{prelude::*, DynMsgReceiverList};
use odin_actor::errors::Result;
use std::{sync::Arc, future::Future, pin::Pin};
use odin_common::datetime::{millis, secs};

/// example for using publish/subscribe with [`DynMsgReceiverList<T>`]

#[derive(Debug,Clone)] struct Update(u64);

#[derive(Debug)] struct Subscribe(DynMsgReceiver<Update>);

/* #region Updater ********************************************************/

define_actor_msg_set! { UpdaterMsg = Subscribe }

struct Updater {
    subscribers: DynMsgReceiverList<Update>,
    count: u64,
    timer: Option<AbortHandle>
}

impl Updater {
    fn new ()->Self {
        Updater { subscribers: DynMsgReceiverList::new(), count: 0, timer: None }
    }
}

impl_actor! { match msg for Actor<Updater,UpdaterMsg> as
    _Start_ => cont! {
        if let Ok(timer) = self.hself.start_repeat_timer( 1, secs(1), false) {
            self.timer = Some(timer);
            println!("{} started update timer", self.hself.id);
        }
    }
    _Timer_ => {
        self.count += 1;
        if self.count < 5 {
            self.subscribers.send_msg( Update(self.count), true).await;
            ReceiveAction::Continue
        } else {
            println!("{} had enough of it, request termination.", self.hself.id); 
            ReceiveAction::RequestTermination 
        }
    }
    Subscribe => cont! {
        println!("got new subscription: {:?}", msg);
        self.subscribers.push( msg.0)
    }
}

/* #endregion Updater */

/* #region Client ********************************************************/

define_actor_msg_set! { ClientMsg = Update }

struct Client;

impl_actor! { match msg for Actor<Client,ClientMsg> as
    Update => cont! { println!("{} got {:?}", self.hself.id, msg) }
}

/* #endregion Client */


#[tokio::main]
async fn main ()->Result<()> {
    let mut actor_system = ActorSystem::new("main");

    let updater = spawn_actor!( actor_system, "updater", Updater::new())?;
    let client_1 = spawn_actor!( actor_system, "client-1", Client{})?;
    let client_2 = spawn_actor!( actor_system, "client-2", Client{})?;

    updater.send_msg( Subscribe( client_1.into())).await;
    updater.send_msg( Subscribe( client_2.into())).await;

    actor_system.timeout_start_all(millis(20)).await?;
    actor_system.process_requests().await
}