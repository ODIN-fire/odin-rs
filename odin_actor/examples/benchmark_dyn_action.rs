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

/// simple macro-benchmark for callbacks (to compare with benchmark_actions)

mod provider {
    use odin_actor::prelude::*;

    #[derive(Debug)] pub struct TriggerAction{}
    #[derive(Debug)] pub struct AddAction(pub DynDataAction<u64>);

    define_actor_msg_set!{ pub ProviderMsg = AddAction | TriggerAction }

    pub struct Provider {
        data: u64,
        actions: DynDataActionList<u64>,
    }
    impl Provider {
        pub fn new()->Self { Provider{ data: 0, actions: DynDataActionList::new() } }
    }

    impl_actor! { match msg for Actor<Provider,ProviderMsg> as
        AddAction => cont! { 
            self.actions.push( msg.0) 
        }
        TriggerAction => cont! { 
            self.data += 1;
            self.actions.execute( self.data, true).await 
        }
    }
}

mod client {
    use std::time::{Instant,Duration};
    use odin_actor::prelude::*;
    use crate::provider::{ProviderMsg,AddAction,TriggerAction};

    #[derive(Debug)] pub struct Update(u64);
    #[derive(Debug)] pub struct PingSelf(u64);
    #[derive(Debug)] pub struct TryPingSelf(u64);

    define_actor_msg_set!{ pub ClientMsg = PingSelf | TryPingSelf | Update }

    pub struct Client {
        max_rounds: u64,
        provider: ActorHandle<ProviderMsg>,
        start_time: Instant,
        elapsed_ping: Duration,
        elapsed_try_ping: Duration,
    }
    impl Client {
        pub fn new (max_rounds: u64, provider: ActorHandle<ProviderMsg>)->Self {
            Client{ max_rounds, provider, start_time: Instant::now(), elapsed_ping: Duration::new(0,0), elapsed_try_ping: Duration::new(0,0) }
        }
    }

    impl_actor! { match msg for Actor<Client,ClientMsg> as
        _Start_ => cont! {
            //let cb = Callback::from( try_send_msg_callback!( &self.hself, |v:&u64| Update(*v) ));
            let action = dyn_data_action!( let hself: ActorHandle<ClientMsg> = self.hself.clone() =>  |v:u64| Ok( hself.try_send_msg( Update(v))? ));

            self.provider.send_msg( AddAction(action)).await;
            self.start_time = Instant::now();
            self.hself.try_send_msg( TryPingSelf(0));
        }
        TryPingSelf => cont! {
            // measure sync msg send time
            if msg.0 < self.max_rounds {
                self.hself.try_send_msg( TryPingSelf(msg.0 + 1));
            } else {
                self.elapsed_try_ping = Instant::now() - self.start_time;
                println!("time per self try_send_msg roundtrip: {} ns", self.elapsed_try_ping.as_nanos() as u64 / self.max_rounds);

                self.start_time = Instant::now();
                self.hself.send_msg( PingSelf(0)).await;
            }
        }
        PingSelf => cont! {
            // measure async msg send time
            if msg.0 < self.max_rounds {
                self.hself.send_msg( PingSelf(msg.0 + 1)).await;
            } else {
                self.elapsed_ping = Instant::now() - self.start_time;
                println!("time per self send_msg roundtrip: {} ns", self.elapsed_ping.as_nanos() as u64 / self.max_rounds);

                // done measuring raw msg roundtrip, now start callback loop
                self.start_time = Instant::now();
                self.provider.try_send_msg( TriggerAction{});
            }
        }
        Update => {
            if msg.0 < self.max_rounds { 
                self.provider.try_send_msg( TriggerAction{});
                ReceiveAction::Continue 
            } else {
                let elapsed = Instant::now() - self.start_time;
                println!("{} action roundtrips in {} μs -> {} ns/callback", 
                        self.max_rounds, elapsed.as_micros(), (elapsed.as_nanos() as u64 / self.max_rounds));
                println!("action overhead per roundtrip: {} ns", 
                    (elapsed.as_nanos() - self.elapsed_try_ping.as_nanos() - self.elapsed_ping.as_nanos()) as u64/self.max_rounds);
                ReceiveAction::RequestTermination 
            }
        }
    }
}

use tokio;
use std::time::{Instant,Duration};
use odin_actor::prelude::*;
use odin_actor::errors::Result;

#[tokio::main]
async fn main()->Result<()> {
    let max_rounds = get_max_rounds();
    println!("-- running benchmark_dyn_action with {} rounds", max_rounds);

    let mut actor_system = ActorSystem::new("benchmark_dyn_action");
    let prov = spawn_actor!( actor_system, "provider", provider::Provider::new())?;
    let cli = spawn_actor!( actor_system, "client", client::Client::new(max_rounds, prov))?;

    actor_system.timeout_start_all(millis(20)).await?;
    actor_system.process_requests().await?;

    Ok(())
}

fn get_max_rounds()->u64 {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 1 {
        1_000_000 // our default value
    } else {
        args[1].parse().expect("max round argument not an integer")
    }
}