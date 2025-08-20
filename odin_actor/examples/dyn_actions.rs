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

use tokio;
use colored::Colorize;

use odin_actor::prelude::*;
use odin_actor::errors::Result;
use odin_common::datetime::secs;

/// version of actions.rs example that uses dyn actions to dynamically register for updates
/// upon the first connection request and obtains the initial data snapshot via dyn_dataref_action
/// that is send to the Updater wrapped in an ExecuteAction message

/* #region updater ***************************************************************************/

type TUpdate = usize;

struct Updater {
    data: Vec<TUpdate>,
    count: usize,
    update_actions: DynDataActionList<TUpdate>,
}
impl Updater {
    fn new()->Self {
        Updater { data: Vec::new(), count: 0, update_actions: DynDataActionList::new() }
    }

    fn update (&mut self)->TUpdate {
        self.count += 1;
        self.data.push( self.count);
        self.count
    }
}

#[derive(Debug)] struct AddUpdateAction(DynDataAction<TUpdate>);

#[derive(Debug)] struct ExecuteAction(DynDataRefAction<Vec<TUpdate>>);


define_actor_msg_set! { UpdaterMsg = AddUpdateAction | ExecuteAction }

impl_actor! { match msg for Actor<Updater,UpdaterMsg> as
    _Start_ => cont! {
        self.hself.start_repeat_timer( 1, secs(1), false);
        println!("{} started update timer", self.id().white());
    }
    _Timer_ => {
        let update = self.update();
        println!("update cycle {}", update);
        self.update_actions.execute(update, true).await;

        if self.count >= 10 {
            println!("{} had enough of it, request termination.", self.id().white()); 
            ReceiveAction::RequestTermination 
        } else {
            ReceiveAction::Continue
        }
    }
    AddUpdateAction => cont! {
        println!("{} adding new update action {:?}", self.id().white(), msg.0);
        self.update_actions.push( msg.0)
    }
    ExecuteAction => cont! {
        println!("{} received {msg:?}", self.id().white());
        msg.0.execute(&self.data).await;
    }

}

/* #endregion updater */

/* #region WsServer *********************************************************************************/
type TAddr = String;

struct ConnectActionData { hself: ActorHandle<WsServerMsg>, addr: TAddr, is_first: bool }

struct WsServer<A> where A: DataAction<ConnectActionData> {
    connections: Vec<TAddr>,
    connect_action: A
}
impl<A> WsServer<A> where A: DataAction<ConnectActionData> {
    pub fn new (connect_action: A)->Self { WsServer { connections: Vec::new(), connect_action } }
}

#[derive(Debug)] struct PublishUpdate { ws_msg: String } // sent from updater (via action)
#[derive(Debug)] struct SendSnapshot { addr: TAddr, ws_msg: String } // send from updater (via action)
#[derive(Debug)] struct SimulateNewConnectionRequest { addr: TAddr } // simulate external event

define_actor_msg_set! { WsServerMsg = PublishUpdate | SendSnapshot | SimulateNewConnectionRequest }

impl_actor! { match msg for Actor<WsServer<A>,WsServerMsg> where A: DataAction<ConnectActionData> as
    SimulateNewConnectionRequest => cont! {
        println!("{} got new connection request from addr {:?}, executing connect action..", self.id().yellow(), msg.addr);

        let action_data = ConnectActionData { hself: self.hself.clone(), addr: msg.addr.clone(), is_first: self.connections.is_empty() };
        self.connections.push(msg.addr);

        self.connect_action.execute( action_data).await;
    }
    PublishUpdate => cont! {
        if self.connections.is_empty() { 
            println!("{} doesn't have connections yet, ignore received update", self.id().yellow())
        } else {
            println!("{} publishing update '{}' to connection addrs {:?}", self.id().yellow(), msg.ws_msg, self.connections)
        }
    }
    SendSnapshot => cont! {
        println!("{} sending snapshot '{}' to connection '{}'", self.id().yellow(), msg.ws_msg, msg.addr);
    }
}

/* #endregion WsServer */

#[tokio::main]
async fn main ()->Result<()> {
    let mut sys = ActorSystem::new("main");

    let updater = spawn_actor!( sys, "updater", Updater::new())?;

    let ws_server = spawn_actor!( sys, "ws_server", WsServer::new(
        data_action!( let updater: ActorHandle<UpdaterMsg> = updater => |cd: ConnectActionData| {
            let ConnectActionData { hself, addr, is_first } = cd;

             // if this is the first connection register for updates in a format the WsServer understands
            if cd.is_first {
                let action_msg = AddUpdateAction( dyn_data_action! {
                    let hself: ActorHandle<WsServerMsg> = hself.clone() => |data: TUpdate| { // data is from updater
                        let msg = PublishUpdate{ ws_msg: format!("{{\"update\": {data}}}") }; // turn data into JSON message
                        Ok( hself.try_send_msg(msg)? )
                    }
                });
                updater.send_msg( action_msg).await?
            }

            // now ask for a snapshot of the current Updater data in a format the WsServer understands
            let action = dyn_dataref_action!(
                let hself: ActorHandle<WsServerMsg> = hself, 
                let addr: TAddr = addr =>
                |data: &Vec<TUpdate>| {
                    let msg = SendSnapshot{ addr: addr.clone(), ws_msg: format!("{:?}", data) }; // turn data into JSON message
                    Ok( hself.try_send_msg( msg)? )
                }
            );
            Ok( updater.send_msg( ExecuteAction(action)).await? )
        })
    ))?;

    sys.start_all().await?;

    sleep( secs(2)).await;
    ws_server.send_msg( SimulateNewConnectionRequest{addr: "42".to_string()}).await?;

    sleep( secs(3)).await;
    ws_server.send_msg( SimulateNewConnectionRequest{addr: "43".to_string()}).await?;

    sys.process_requests().await?;

    Ok(())
}
