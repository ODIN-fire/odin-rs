/*
 * Copyright (c) 2024, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The ODIN - Open Data Integration Framework is licensed under the
 * Apache License, Version 2.0 (the "License"); you may not use this file
 * except in compliance with the License. You may obtain a copy of the
 * License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
#![allow(unused)]

use tokio;
use std::collections::VecDeque;
use std::{future::Future,sync::Arc};
use odin_actor::prelude::*;
use odin_actor::errors::Result;

/* #region updater ***************************************************************************/

// note - updater does not need to know anything about potential clients - it only feeds
// its data into provided callbacks
// Note also that async callbacks are not particularly efficient since they have to
// wrap opaque futures on each invocation. This is mostly tolerable (for now) because
// high frequent (update) callback executions probably use the sync try_send_msg_callback
// if the update data has a short lifespan

struct Updater {
    data: Vec<String>,
    count: usize,
    update_action: DynDataActionList<String>,
}
impl Updater {
    fn new()->Self {
        Updater { data: Vec::new(), count: 0, update_action: DynDataActionList::new() }
    }

    fn update (&mut self)->bool {
        let new_value = format!("{} Missisippi", self.count);
        self.data.push( new_value);
        true
    }
}

#[derive(Debug)] struct AddUpdateAction(DynDataAction<String>);

#[derive(Debug)] struct ExecuteAction(Arc<DynDataAction<Vec<String>>>);
impl ExecuteAction {
    pub async fn execute (&self, data: &Vec<String>)->Result<()> {
        self.0.execute(data).await
    }
}

define_actor_msg_set! { UpdaterMsg = AddUpdateAction | ExecuteAction }

impl_actor! { match msg for Actor<Updater,UpdaterMsg> as
    _Start_ => cont! {
        self.hself.start_repeat_timer( 1, secs(1));
        println!("{} started update timer", self.hself.id);
    }
    _Timer_ => {
        self.count += 1;
        println!("update cycle {}", self.count);
        if self.update() {
            self.update_action.execute( self.data.last().unwrap()).await;
        }

        if self.count >= 5 {
            println!("{} had enough of it, request termination.", self.hself.id); 
            ReceiveAction::RequestTermination 
        } else {
            ReceiveAction::Continue
        }
    }
    AddUpdateAction => cont! {
        self.update_action.push( msg.0)
    }
    ExecuteAction => cont! {
        println!("updater received {msg:?}");
        msg.execute(&self.data).await;
    }

}

/* #endregion updater */

/* #region server *********************************************************************************/
struct WsServer {} 

// these message types are too 'WsServer' specific to be forced upon a generic, reusable Updater

#[derive(Debug)] struct PublishWsMsg { ws_msg: String }

#[derive(Debug)] struct SendWsMsg { addr: &'static str, ws_msg: String }

define_actor_msg_set! { WsServerMsg = PublishWsMsg | SendWsMsg }

impl_actor! { match msg for Actor<WsServer,WsServerMsg> as
    PublishWsMsg => cont! {
        println!("WsServer publishing data '{}' to all its connections", msg.ws_msg);
    }
    SendWsMsg => cont! {
        println!("WsServer sending data '{}' to connection '{}'", msg.ws_msg, msg.addr);
    }
}

/* #endregion server */

#[tokio::main]
async fn main ()->Result<()> {
    let mut actor_system = ActorSystem::new("main");

    let updater = spawn_actor!( actor_system, "updater", Updater::new())?;
    let server = spawn_actor!( actor_system, "server", WsServer{}, 4)?;

    // note how we construct the DynDataAction from a mix of captured sender/local (server, addr) and passed-in receiver/remote data
    let addr = "fortytwo";
    let on_demand_action = Arc::new( send_msg_dyn_action!(server, |v: &Vec<String>| SendWsMsg{addr, ws_msg: format!("{v:?}")}));
    updater.send_msg( ExecuteAction( on_demand_action.clone())).await?; // we send multiple times so we have to clone

    let update_action = try_send_msg_dyn_action!(server, |v: &String| PublishWsMsg{ws_msg: format!("{{\"update\": {v:?}}}")});
    updater.send_msg( AddUpdateAction(update_action)).await?;

    actor_system.timeout_start_all(millis(20)).await?;

    sleep( secs(2)).await;

    updater.send_msg( ExecuteAction( on_demand_action)).await?;

    actor_system.process_requests().await?;

    Ok(())
}
