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

use odin_actor::{prelude::*, DynMsgReceiverList, MsgReceiverList};
use odin_actor::errors::Result;
use std::{sync::Arc, future::Future, pin::Pin};
use odin_common::datetime::{millis, secs};

/// example for static publish/subscribe using [`MsgReceiverList<T>`]`

#[derive(Debug,Clone)] struct Update(u64);

/* #region Updater ********************************************************/

define_actor_msg_set! { UpdaterMsg }

struct Updater<L> where L: MsgReceiverList<Update> {
    subscribers: L,
    count: u64,
    timer: Option<AbortHandle>
}

impl<L> Updater<L> where L: MsgReceiverList<Update> {
    fn new (subscribers: L)->Self {
        Updater { subscribers, count: 0, timer: None }
    }
}

impl_actor! { match msg for Actor<Updater<L>,UpdaterMsg> where L: MsgReceiverList<Update> as
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
}

/* #endregion Updater */

/* #region Clients ********************************************************/

#[derive(Debug)] struct Foo {}
define_actor_msg_set! { Client1Msg = Update | Foo }

struct Client1;

impl_actor! { match msg for Actor<Client1,Client1Msg> as
    Update => cont! { println!("{} got {:?}", self.id(), msg) }
    Foo => cont! { println!("{} got Foo", self.id())}
}


#[derive(Debug)] struct Bar {}
define_actor_msg_set! { Client2Msg = Update | Bar }

struct Client2;

impl_actor! { match msg for Actor<Client2,Client2Msg> as
    Update => cont! { println!("{} got {:?}", self.id(), msg) }
    Bar => cont! { println!("{} got Bar", self.id())}
}

/* #endregion Clients */


#[tokio::main]
async fn main ()->Result<()> {
    let mut actor_system = ActorSystem::new("main");

    let client_1 = spawn_actor!( actor_system, "client-1", Client1{})?;
    let client_2 = spawn_actor!( actor_system, "client-2", Client2{})?;

    let updater = spawn_actor!( actor_system, "updater", Updater::new( msg_receiver_list!( client_1, client_2 : MsgReceiver<Update>)))?;

    actor_system.timeout_start_all(millis(20)).await?;
    actor_system.process_requests().await
}