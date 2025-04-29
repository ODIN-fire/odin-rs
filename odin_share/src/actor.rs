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

use odin_common::{arc};
use odin_action::{DataAction,DynDataRefAction,OdinActionFailure};
use odin_actor::prelude::*;
use odin_actor::errors;
use odin_server::prelude::*;
use odin_build::pkg_data_dir;

use std::marker::PhantomData;
use std::{ collections::HashMap, path::Path, fs::File, io::BufReader, io, fmt::Debug, sync::Arc, result::Result };
use serde::{Serialize,Deserialize};
use serde_json;
use odin_common::fs;

use crate::errors::{OdinShareError,OdinShareResult,op_failed};
use crate::{
    SharedStore,SharedStoreReadAccess,SharedStoreAction,DynSharedStoreAction,SharedStoreValueConstraints,PersistentHashMapStore,
    shared_store_action,
    share_service::{SharedItemType,ShareService,SetShared,RemoveShared}
};

/// action argument type to announce changes to clients of a SharedStore. Note this does not include the changed value, 
/// which might be expensive to clone.
#[derive(Debug,Clone)]
pub struct SharedStoreChange<'a,T> where T: SharedStoreValueConstraints {
    pub update: SharedStoreUpdate<T>, // the sendable part
    pub store: &'a dyn SharedStoreReadAccess<T> // the non-sendable store reference that can be used to retrieve store values from action bodies
}

/// a sendable part for a SharedStore change, in case we need an actor message. This specifies the nature of the change
/// but does not include changed values (they have to be queried by recipients)
#[derive(Debug,Clone)]
pub enum SharedStoreUpdate<T> where T: SharedStoreValueConstraints {
    Set { hstore: ActorHandle<SharedStoreActorMsg<T>>, key: String },
    Remove { hstore: ActorHandle<SharedStoreActorMsg<T>>, key: String },
}

/// the state of an actor that encapsulates a SharedStore impl
pub struct SharedStoreActor<T,S,I,C> 
    where 
        T: SharedStoreValueConstraints, 
        S: SharedStore<T>, 
        I: SharedStoreAction<T> + Send, 
        C: for<'a> DataAction<SharedStoreChange<'a, T>>
{
    store: S,
    init_action: I,
    change_action: C,

    phantom_t: PhantomData<T>
}

impl <T,S,I,C> SharedStoreActor<T,S,I,C> 
    where 
        T: SharedStoreValueConstraints, 
        S: SharedStore<T>, 
        I: SharedStoreAction<T> + Send, 
        C: for<'a> DataAction<SharedStoreChange<'a, T>>
{
    pub fn new (store: S, init_action: I, change_action: C)->Self {
        SharedStoreActor { store, init_action, change_action, phantom_t: PhantomData }
    }

    async fn initialize (&mut self)->OdinShareResult<()> {
        self.store.initialize().await?;
        self.init_action.execute( &self.store as &dyn SharedStore<T>).await.map_err(|e| op_failed("init action failed {e}"))
    }

    async fn set (&mut self, hself: ActorHandle<SharedStoreActorMsg<T>>, key: String, value: T) {
        if self.change_action.is_empty() {
            self.store.insert( key, value);
        } else {
            self.store.insert( key.clone(), value);
            let update = SharedStoreUpdate::Set{ hstore: hself, key: key };
            self.change_action.execute( SharedStoreChange{update,store: &self.store}).await;
        }
    }

    async fn remove (&mut self, hself: ActorHandle<SharedStoreActorMsg<T>>, key: String) {
        if self.change_action.is_empty() {
            self.store.remove( &key);
        } else {
            let update = SharedStoreUpdate::Remove{ hstore: hself, key: key.clone() };
            self.store.remove( &key);
            self.change_action.execute( SharedStoreChange{update,store: &self.store}).await;
        }
    }
}

//--- messages

#[derive(Debug)] 
pub struct SetSharedStoreEntry<T> {
    pub key: String,
    pub value: T
}

#[derive(Debug)] 
pub struct RemoveSharedStoreEntry {
    pub key: String
}

#[derive(Debug)] 
pub struct ExecSnapshotAction<T>( pub DynSharedStoreAction<T> );

define_actor_msg_set! { pub SharedStoreActorMsg<T> where T: SharedStoreValueConstraints = 
    SetSharedStoreEntry<T> | RemoveSharedStoreEntry | Query<String,Option<T>> | ExecSnapshotAction<T>
}


impl_actor! { match msg for Actor<SharedStoreActor<T,S,I,C>,SharedStoreActorMsg<T>> 
    where 
        T: SharedStoreValueConstraints, 
        S: SharedStore<T>, 
        I: SharedStoreAction<T> + Send, 
        C: for<'a> DataAction<SharedStoreChange<'a, T>>
    as
    _Start_ => cont! {
        if let Err(e) = self.state.initialize().await {
            error!("store failed to initialize {e}");
        }
    }

    SetSharedStoreEntry<T> => cont! {
        let hself = self.hself.clone();
        self.state.set( hself, msg.key, msg.value).await;
    }
    RemoveSharedStoreEntry => cont! {
        let hself = self.hself.clone();
        self.state.remove( hself, msg.key).await;
    }
    Query<String,Option<T>> => cont! {
        msg.respond( self.state.store.get(&msg.question).map(|vr| vr.clone())).await;
    }
    ExecSnapshotAction<T> => cont! {
        msg.0.execute( &self.state.store as &dyn SharedStore<T>).await;
    }
}

/// spawn a persistent share actor that sends shared item updates to the provided SpaServer.
/// The shared store is initialized from, and optionally writes to, `ODIN_ROOT/data/odin_share/shared_items.json`.
/// 
/// There is no reason a SharedStoreActor cannot be used by other actors within an ODIN actor system but - since shared items
/// are normally created by users - the primary use case is to provide the storage backend for a SpaServer. We provide this
/// method to set up required init and change actions to avoid duplicated boilerplate code in applications
pub fn spawn_server_share_actor (actor_system: &mut ActorSystem, name: &str, hserver: ActorHandle<SpaServerMsg>, path: impl AsRef<Path>, save:bool)->OdinShareResult<ActorHandle<SharedStoreActorMsg<SharedItemType>> >
{
    let store_name = arc!(name);
    let store = PersistentHashMapStore::new( &path, save)?;

    let actor_state = SharedStoreActor::new( 
        store, 
        shared_store_action!( 
            let hserver: ActorHandle<SpaServerMsg> = hserver.clone(),
            let sender_id: Arc<String> = store_name.clone() => 
            |store as &dyn SharedStore<SharedItemType>| announce_data_availability( &hserver, sender_id).await
        ),
        data_action!( let hserver: ActorHandle<SpaServerMsg> = hserver.clone() => 
            |update: SharedStoreChange<'_,SharedItemType>| broadcast_store_change( &hserver, update).await
        )
    );

    Ok( spawn_actor!( actor_system, &store_name, actor_state)? )
}

/// helper function for the body of a SharedStore init action
/// This just announces data avaiability by sending a message to the provided SpaServer actor handle
pub async fn announce_data_availability<'a> (hserver: &'a ActorHandle<SpaServerMsg>, sender_id: &Arc<String>)->Result<(),OdinActionFailure> {
    hserver.send_msg( DataAvailable::new::<SharedItemType>( sender_id) ).await.map_err(|e| e.into())
}

/// helper function for the body of a SharedStoreActor change action
/// This sends change-specific websocket messages to all connected clients of the provided SpaServer actor  
pub async fn broadcast_store_change<'a> (hserver: &'a ActorHandle<SpaServerMsg>, change: SharedStoreChange<'a,SharedItemType>)->Result<(),OdinActionFailure> {
    match change.update {
        SharedStoreUpdate::Set { hstore, key } => {
            if let Some(stored_val) = change.store.get( &key) {
                let msg = SetShared{key, value: stored_val.clone()};
                if let Ok(data) = WsMsg::json( ShareService::mod_path(), "setShared", msg) {
                    hserver.send_msg( BroadcastWsMsg{ws_msg: data}).await?;
                }
            }
        }
        SharedStoreUpdate::Remove { hstore, key } => {
            let msg = RemoveShared{key};
            if let Ok(data) = WsMsg::json( ShareService::mod_path(), "removeShared", msg) {
                hserver.send_msg( BroadcastWsMsg{ws_msg: data}).await?;
            }
        }
        _ => {}
    }

    Ok(())
}

