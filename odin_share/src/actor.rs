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

use odin_action::{DataAction,DynDataRefAction};
use odin_actor::prelude::*;
use odin_actor::errors;

use std::marker::PhantomData;
use std::{ collections::HashMap, path::Path, fs::File, io::BufReader, io, fmt::Debug };
use serde::{Serialize,Deserialize};
use serde_json;
use odin_common::fs;

use crate::errors::op_failed;
use crate::errors::OdinShareError;
use crate::{SharedStore,SharedStoreAction,DynSharedStoreAction,SharedStoreValueConstraints};

/// message type to announce changes to clients of a SharedStore. Note this does not include the
/// changed value, which might be expensive to clone
#[derive(Debug,Clone)]
pub enum SharedStoreChange<T> where T: SharedStoreValueConstraints {
    Set { hstore: ActorHandle<SharedStoreActorMsg<T>>, key: String },
    Remove { hstore: ActorHandle<SharedStoreActorMsg<T>>, key: String },
    // TODO - add more ops, like Rename
}

/// the state of an actor that encapsulates a SharedStore impl
pub struct SharedStoreActor<T,S,I,C> where T: SharedStoreValueConstraints, S: SharedStore<T>, I: SharedStoreAction<T> + Send, C: DataAction<SharedStoreChange<T>> {
    store: S,
    init_action: I,
    change_action: C,

    phantom_t: PhantomData<T>
}

impl <T,S,I,C> SharedStoreActor<T,S,I,C> 
    where T: SharedStoreValueConstraints, S: SharedStore<T>, I: SharedStoreAction<T> + Send, C: DataAction<SharedStoreChange<T>>
{
    pub fn new (store: S, init_action: I, change_action: C)->Self {
        SharedStoreActor { store, init_action, change_action, phantom_t: PhantomData }
    }

    async fn initialize (&mut self)->Result<(),OdinShareError> {
        self.store.initialize().await?;
        self.init_action.execute( &self.store as &dyn SharedStore<T>).await.map_err(|e| op_failed("init action failed {e}"))
    }

    async fn set (&mut self, hself: ActorHandle<SharedStoreActorMsg<T>>, key: String, value: T) {
        if self.change_action.is_empty() {
            self.store.insert( key, value);
        } else {
            let change = SharedStoreChange::Set{ hstore: hself, key: key.clone() };
            self.store.insert( key, value);
            self.change_action.execute(change).await;
        }
    }

    async fn remove (&mut self, hself: ActorHandle<SharedStoreActorMsg<T>>, key: String) {
        if self.change_action.is_empty() {
            self.store.remove( &key);
        } else {
            let change = SharedStoreChange::Remove{ hstore: hself, key: key.clone() };
            self.store.remove( &key);
            self.change_action.execute(change).await;
        }
    }
}

//--- messages

#[derive(Debug)] 
pub struct SetSharedStoreValue<T> {
    pub key: String,
    pub value: T
}

#[derive(Debug)] 
pub struct RemoveSharedStoreValue {
    pub key: String
}

#[derive(Debug)] 
pub struct ExecSnapshotAction<T>( pub DynSharedStoreAction<T> );

define_actor_msg_set! { pub SharedStoreActorMsg<T> where T: SharedStoreValueConstraints = 
    SetSharedStoreValue<T> | RemoveSharedStoreValue | Query<String,Option<T>> | ExecSnapshotAction<T>
}


impl_actor! { match msg for Actor<SharedStoreActor<T,S,I,C>,SharedStoreActorMsg<T>> 
        where T: SharedStoreValueConstraints, S: SharedStore<T>, I: SharedStoreAction<T> + Send, C: DataAction<SharedStoreChange<T>> as
    _Start_ => cont! {
        if let Err(e) = self.state.initialize().await {
            error!("store failed to initialize {e}");
        }
    }

    SetSharedStoreValue<T> => cont! {
        let hself = self.hself.clone();
        self.state.set( hself, msg.key, msg.value).await;
    }
    RemoveSharedStoreValue => cont! {
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
