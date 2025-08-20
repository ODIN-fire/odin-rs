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

/// example of how to connect two actors that don't have to know about each others message interfaces.
///
/// The decoupling is achieved through construction-site defined ActionLists (set up in `main()`).
/// ActionList<ProviderData> is triggered by state changes in the provider
/// Action2List<ProviderData,ClientData> is triggered by the client
/// 
/// The underlying model is an async updated data source actor as the provider (e.g. for tracking objects)
/// that implements two abstract interaction points:
///   - update actions when its internal data model changes
///   - snapshot actions triggered by clients, passing in additional client request data
///
/// Note the provider implementation does not need to know the specific ActorHandles/messages of clients
/// and hence also does not need to know about potential data transformation to create such action messages. The
/// only thing it needs to know is when to trigger respective actions and what data to pass into respective
/// `execute(..}` invocations for these actions.
///
/// The client models a web server that receives external connection requests, upon which it
/// needs to send snapshot data that is created through provider callback actions.
/// Once a new connection is established the web server then sends whatever it receives through the provider
/// update actions to all live connections.
/// Note there is nothing in the client that needs to know the concrete provider message interface or its
/// update/snapshot data types. All this information is encapsulated in `ActionList` instances at the
/// system construction site (`main()`)

use tokio;
use odin_actor::prelude::*;
use odin_actor::errors::Result;
use odin_common::datetime::secs;
use colored::Colorize;

//-- to make semantics more clear
type TProviderUpdate = i64;
type TProviderSnapshot = Vec<TProviderUpdate>;
type TRequest = String;

/* #region provider ***************************************************************************/

/// provider example, modeling some async changed data store (tracks, sensor readings etc.)
struct Provider<A1,A2> where A1: DataAction<TProviderUpdate>, A2: BiDataRefAction<TProviderSnapshot,TRequest>
{
    data: TProviderSnapshot,
    count: usize,

    update_actions: A1, // actions to be triggered when our data changes
    snapshot_action: A2 // actions to be triggered when a client requests a snapshot
}
impl<A1,A2> Provider<A1,A2> where A1: DataAction<TProviderUpdate>, A2: BiDataRefAction<TProviderSnapshot,TRequest>
{
    fn new(update_action: A1, snapshot_action: A2)->Self {
        Provider { data: Vec::new(), count: 0, update_actions: update_action, snapshot_action }
    }

    fn update(&mut self) -> TProviderUpdate {
        let update_data = self.count as TProviderUpdate;
        self.data.push( update_data);
        update_data
    }
}

#[derive(Debug)] struct ExecSnapshotAction { request: TRequest }

define_actor_msg_set! { ProviderMsg = ExecSnapshotAction }

impl_actor! { match msg for Actor<Provider<A1,A2>,ProviderMsg> 
                    where A1: DataAction<TProviderUpdate>, A2: BiDataRefAction<TProviderSnapshot,TRequest> as
    _Start_ => cont! {
        self.hself.start_repeat_timer( 1, secs(1), false);
        println!("{} started", self.id().white());
    }
    _Timer_ => { // simulate async change of data (e.g. through some external I/O)
        self.count += 1;
        println!("{} update cycle {}", self.id().white(), self.count);
        let update_data = self.update();

        if self.count < 10 {
            self.update_actions.execute( update_data).await;
            ReceiveAction::Continue
        } else {
            println!("{} had enough of it, request termination.", self.id().white()); 
            ReceiveAction::RequestTermination 
        }
    }
    ExecSnapshotAction => cont! { // client requests a full data snapshot - pass on the label of the request
        println!("{} received {msg:?}", self.id().white());
        self.snapshot_action.execute( &self.data, msg.request).await;
    }

}

/* #endregion provider */

/* #region client *********************************************************************************/

type TAddr = String; // used as action bi_data, i.e. has to be the same as TLabel in the Provider

/// client example, modeling a web server that manages web socket connections
pub struct WsServer<A> where A: DataAction<TAddr> {
    connections: Vec<TAddr>,
    new_request_action: A // action to be triggered when WsServer gets a new (external) connection request
}
impl <A> WsServer<A> where A: DataAction<TAddr> {
    pub fn new (new_request_action: A)->Self { WsServer{connections: Vec::new(), new_request_action} }
}

// these message types are too 'WsServer' specific to be forced upon a generic, reusable Provider

#[derive(Debug)] struct PublishUpdate { ws_msg: String }

#[derive(Debug)] struct SendSnapshot { addr: TAddr, ws_msg: String }

#[derive(Debug)] struct SimulateNewConnectionRequest { addr: TAddr }

define_actor_msg_set! { WsServerMsg = PublishUpdate | SendSnapshot | SimulateNewConnectionRequest }

impl_actor! { match msg for Actor<WsServer<A>,WsServerMsg> where A: DataAction<TAddr> as
    SimulateNewConnectionRequest => cont! { // mockup simulating a new external connection event from 'addr'
        // note we don't add msg.addr to connections yet since that could cause sending updates before init snapshots
        println!("{} got new connection request from addr {:?}", self.id().yellow(), msg.addr);
        self.new_request_action.execute( msg.addr).await;
    }
    PublishUpdate => cont! {
        if self.connections.is_empty() { 
            println!("{} doesn't have connections yet, ignore received data update", self.id().yellow())
        } else {
            println!("{} publishing data update '{}' to connection addrs {:?}", self.id().yellow(), msg.ws_msg, self.connections)
        }
    }
    SendSnapshot => cont! {
        self.connections.push(msg.addr.clone());
        println!("{} sending snapshot data '{}' to connection addr '{}'", self.id().yellow(), msg.ws_msg, msg.addr);
    }
}

/* #endregion client */

#[tokio::main]
async fn main ()->Result<()> {
    let mut actor_system = ActorSystem::new("main");
    let provider = PreActorHandle::new( &actor_system, "provider", 8); // we need it to construct the client

    //--- set up the client
    let client = spawn_actor!( actor_system, "client", 
        WsServer::new( 
            data_action!( let provider: ActorHandle<ProviderMsg> = provider.to_actor_handle() => 
                              |addr:TAddr| Ok( provider.try_send_msg( ExecSnapshotAction{request: addr})? ))
        )
    )?;

    //--- set up the provider
    let provider = spawn_pre_actor!( actor_system, provider, 
        Provider::new(
            data_action!( let client: ActorHandle<WsServerMsg> = client.clone() => |data: TProviderUpdate| {
                let msg = PublishUpdate{ws_msg: format!("{{\"update\": \"{data}\"}}")}; // construct client message from provider data
                Ok( client.try_send_msg( msg)? )
            }),
            bi_dataref_action!( let client: ActorHandle<WsServerMsg> = client.clone() => |data: &TProviderSnapshot, req:TRequest| {
                let msg = SendSnapshot{ addr: req, ws_msg: format!("{data:?}") }; // construct client message from label and provider data ref
                Ok( client.try_send_msg( msg)? )
            })     
        )
    )?;


    actor_system.start_all().await?;

    //--- 3: actor system running - now simulate external requests
    sleep( secs(2)).await;
    client.send_msg( SimulateNewConnectionRequest{addr: "42".to_string()}).await?;

    sleep( secs(3)).await;
    client.send_msg( SimulateNewConnectionRequest{addr: "43".to_string()}).await?;

    actor_system.process_requests().await?;

    Ok(())
}
