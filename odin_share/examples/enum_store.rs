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
use odin_actor::errors::Result;
use serde::{Serialize,Deserialize};
use std::{sync::Arc, collections::HashMap};
use odin_common::datetime::secs;
use odin_share::prelude::*;

//--- the data to be stored

#[derive(Debug,Clone,Serialize,Deserialize)] 
struct Point2D { x: f64, y: f64, comment: String }

#[derive(Debug,Clone,Serialize,Deserialize)] 
struct Point3D { x: f64, y: f64, z: f64, comment: String }

// we use Arc to show how to handle expensive payload structs
#[derive(Debug,Clone,Serialize,Deserialize)] 
enum StoreItem {
    Point2D(Arc<Point2D>),
    Point3D(Arc<Point3D>)
}

//--- the actor that updates the store

struct Updater {
    hstore: ActorHandle<SharedStoreActorMsg<StoreItem>>
}

#[derive(Debug)] struct Ping {}

define_actor_msg_set! { UpdaterMsg = Ping }

impl_actor! { match msg for Actor<Updater,UpdaterMsg> as 
    _Start_ => cont! {
        let value = StoreItem::Point2D(
            Arc::new( Point2D{ x: 42.0, y: -121.0, comment: "this is the middle of nowhere".into() } )
        );
        let update = SetSharedStoreEntry { key: "/location/p1".into(), value };
        println!("updater sending message to store: {update:?}");
        self.hstore.send_msg( update).await;
        self.hself.send_msg( Ping{} ).await;
    }
    Ping => cont! {
        let value = StoreItem::Point3D(
            Arc::new( Point3D{ x: 37.0, y: -122.0, z: 100000.0, comment: "somewhere above the Bay Area".into() } )
        );
        let update = SetSharedStoreEntry { key: "/view/bay_area".into(), value };
        println!("updater sending message to store: {update:?}");
        self.hstore.send_msg( update).await;
    }
}

//--- the client actor that listens to store changes and reads its values

struct Client {}

#[derive(Debug)] struct CheckStore (ActorHandle<SharedStoreActorMsg<StoreItem>>);

define_actor_msg_set! { ClientMsg = SharedStoreUpdate<StoreItem> | CheckStore }

impl_actor! { match msg for Actor<Client,ClientMsg> as
    SharedStoreUpdate<StoreItem> => cont! {
        match msg {
            SharedStoreUpdate::Set{ hstore, key } => {
                println!("client received update for key: {:?}, now querying value..", key);
                match timeout_query_ref( &hstore, key, secs(1)).await {
                    Ok(response) => match response {
                        Some( value ) => {
                            match value {
                                StoreItem::Point2D(p) => {
                                    println!("got 2D value: {p:?}");
                                }
                                StoreItem::Point3D(p) => {
                                    println!("got 3D value: {p:?}");
                                    self.hself.send_msg( CheckStore(hstore) ).await;
                                }
                            }
                        }
                        _ => println!("no item for key found")
                    }
                    Err(e) => println!("query error: {e:?}")
                }
            }
            other => println!("ignoring {other:?}")
        }
    }
    CheckStore => term! {
        println!("now sending ExecSnapshotAction to store..");

        let action = dyn_shared_store_action!( => |store as &dyn SharedStore<StoreItem>| {
            println!("exec snapshot action:");
            for (k,v) in store.ref_iter() {
                println!("{k:?} = {v:?}");
            }
            Ok(())
        });
        msg.0.send_msg( ExecSnapshotAction(action)).await;
    }
}

//--- the system construction

run_actor_system!( asys => {
    let client = PreActorHandle::new( &asys, "client", 8); 

    let hstore = spawn_actor!( asys, "store", SharedStoreActor::new(
        HashMap::new(),
        no_shared_store_action(),
        data_action!( let client: ActorHandle<ClientMsg> = client.to_actor_handle() => 
            |change: SharedStoreChange<'_,StoreItem>| Ok( client.try_send_msg( change.update)? )
        )
    ))?;

    let updater = spawn_actor!( asys, "updater", Updater{hstore})?;

    let client = spawn_pre_actor!( asys, client, Client{})?;
    
    Ok(())
});