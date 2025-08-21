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

#![allow(unused)]

/// same example as alist.rs but this time with a retry_send_msg() if the provider cannot
/// send to the client because it's queue is full.
/// 
/// the main change is that the client 'SimulateNewRequest' and associated action list ('new_request_action')
/// now become regular functional features, i.e. the client provides a 'ExecNewRequest' method that
/// triggers a new (configured) provider request. This message is what we send from the provider snapshot_action
/// instance if the receiver (client) queue is full. This kills two birds with one stone: 
///
///   (a) we make sure that if a ExecSnapshotAction finally succeeds it sends the client the up-to-date data, and 
///   (b) the retry does not have to clone a potentially large message (ExecNewRequest instead of SendSnapshot)

use tokio;
use odin_actor::prelude::*;
use odin_actor::errors::Result;
use odin_common::datetime::{millis,secs};
use colored::Colorize;

//-- to make semantics more clear
type TProviderUpdate = i64;
type TProviderSnapshot = Vec<TProviderUpdate>;
type TRequest = String;
type TAddr = String; // client data

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
    fn new(update_actions: A1, snapshot_action: A2)->Self {
        Provider { data: Vec::new(), count: 0, update_actions, snapshot_action }
    }

    fn update(&mut self) -> TProviderUpdate {
        let update_data = self.count as TProviderUpdate;
        self.data.push( update_data);
        update_data
    }
}

#[derive(Debug)] struct ExecSnapshotAction { client_data: TRequest }

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
    ExecSnapshotAction => cont! { // client requests a full data snapshot
        println!("{} received {msg:?}", self.id().white());
        self.snapshot_action.execute( &self.data, msg.client_data).await;
    }

}

/* #endregion provider */

/* #region client *********************************************************************************/

/// client example, modeling a web server that manages web socket connections
pub struct WsServer<A> where A: DataAction<TAddr> {
    connections: Vec<TAddr>,
    new_request_action: A,// action to be triggered when server gets a new (external) connection request

    n_exec: usize
}
impl <A> WsServer<A> where A: DataAction<TAddr> {
    pub fn new (new_request_action: A)->Self { WsServer{connections: Vec::new(), new_request_action, n_exec:0} }
}

// these message types are too 'WsServer' specific to be forced upon a generic, reusable Provider

#[derive(Debug)] struct PublishUpdate { ws_msg: String }

#[derive(Debug)] struct SendSnapshot { addr: TAddr, ws_msg: String }

#[derive(Debug,Clone)] struct ExecNewRequest { addr: TAddr }

#[derive(Debug)] struct DelayMsg{} // just used to flood the WsServer queue and create backpressure

#[derive(Debug)] struct FloodMsg{} // just used to flood the WsServer queue and create backpressure

define_actor_msg_set! { WsServerMsg = PublishUpdate | SendSnapshot | ExecNewRequest | DelayMsg | FloodMsg }

impl_actor! { match msg for Actor<WsServer<A>,WsServerMsg> where A: DataAction<TAddr> as
    ExecNewRequest => cont! { // mockup simulating a new external connection event from 'addr'
        println!("{} send connection request for {:?}", self.id().yellow(), msg.addr);

        self.n_exec += 1;
        if self.n_exec == 2 { self.hself.try_send_msg( DelayMsg{}); }
        
        self.new_request_action.execute( msg.addr).await;
    }
    PublishUpdate => cont! {
        if self.connections.is_empty() { 
            println!("{} doesn't have connections yet, ignore data update", self.id().yellow())
        } else {
            println!("{} publishing data '{}' to connections {:?}", self.id().yellow(), msg.ws_msg, self.connections)
        }
    }
    SendSnapshot => cont! {
        self.connections.push(msg.addr.clone());
        println!("{} sending snapshot data '{}' to connection '{}'", self.id().yellow(), msg.ws_msg, msg.addr);
    }

    //--- these are just traffic simulators
    DelayMsg => cont! { 
        self.hself.send_msg(FloodMsg{}).await; // make sure there is something in the queue before we delay our receiver loop
        println!("{} doing something lengthy..", self.id().red());
        sleep( secs(1)).await
    }
    FloodMsg => cont! {
        // nothing to do here - this is just a message to flood our queue
    }
}

/* #endregion client */

#[tokio::main]
async fn main ()->Result<()> {
    let mut actor_system = ActorSystem::new("main");
    let provider = PreActorHandle::new( &actor_system, "provider", 8); // we need it to construct the client

    //--- 1: set up the client (WsServer)
    let client = spawn_actor!( actor_system, "client", 
        WsServer::new( 
            data_action!( let provider: ActorHandle<ProviderMsg> = provider.to_actor_handle() => 
                              |addr:TAddr| Ok( provider.try_send_msg( ExecSnapshotAction{client_data: addr.clone()})? ))
        ),
        1 // give the actor a really small queue so that we can saturate it
    )?;

    //--- 2: set up the provider (data source)
    let provider = spawn_pre_actor!( actor_system, provider, 
        Provider::new(
            data_action!( let client: ActorHandle<WsServerMsg> = client.clone() => |data: TProviderUpdate| {
                let msg = PublishUpdate{ws_msg: format!("{{\"update\": \"{data}\"}}")}; // construct client message from provider data
                Ok( client.try_send_msg( msg)? )
            }),
            bi_dataref_action!( let client: ActorHandle<WsServerMsg> = client.clone() => |data: &TProviderSnapshot, req:TRequest| {
                let addr = req.clone();
                let msg = SendSnapshot{ addr: req, ws_msg: format!("{data:?}") }; // construct client message from label and provider data ref
                match client.try_send_msg( msg) {
                    Err(OdinActorError::ReceiverFull) => {
                        println!("{} queue full, retry..", client.id.red());
                        // this is a critical msg - retry if it failed. While we could directly resend the SendSnapshot()
                        // to the client this would be suboptimal since the provider data has most likely changed
                        // at the time this will succeed, which means all updates in-between original request and success
                        // would be lost. Just sending a control message to the client also means we don't have to clone
                        // a potentially huge message
                        Ok( client.retry_send_msg( 5, millis(300), ExecNewRequest{addr})? )
                    }
                    other => Ok( other? )
                }
            })     
        )
    )?;

    actor_system.start_all().await?;

    //--- 3: actor system running - now simulate external requests
    sleep( secs(2)).await;
    client.send_msg( ExecNewRequest{addr: "42".to_string()}).await?;

    sleep( secs(3)).await;
    client.send_msg( ExecNewRequest{addr: "43".to_string()}).await?;

    actor_system.process_requests().await?;

    Ok(())
}
