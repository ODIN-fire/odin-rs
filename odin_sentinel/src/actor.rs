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

#![allow(unused,private_interfaces)]
#![feature(trait_alias)]

use odin_actor::prelude::*;
use odin_actor::{error,debug,warn,info};
use odin_common::{datetime::duration_since, geo::GeoPoint4};
use crate::*;
use crate::ws::WsCmd;


//-- external messages (from other actors)

#[derive(Debug)] pub struct ExecSnapshotAction( pub DynDataRefAction<SentinelStore> );

/// request a specific update record
#[derive(Debug)] pub struct GetSentinelUpdate {  pub record_id: String }

/// retrieve device position
#[derive(Debug)] pub struct GetSentinelPosition {  pub device_id: String, pub date: DateTime<Utc> }

// the answer for a GetSentinelPosition query


/// send a command to Sentinel devices
#[derive(Debug)] pub struct SendSentinelCmd { sentinel_cmd: WsCmd }

//-- internal messages. Note these are not public since we should only get them from our connector
#[derive(Debug)] pub(crate) struct InitializeStore (pub(crate) SentinelStore);  // set initial store contents
#[derive(Debug)] pub(crate) struct UpdateStore (pub(crate) SentinelUpdate); // single record update (triggered by websocket notification)
#[derive(Debug)] pub(crate) struct ConnectorError (pub(crate) OdinSentinelError);

define_actor_msg_set! { pub SentinelActorMsg = 
    //-- messages we get from other actors
    ExecSnapshotAction |
    Query<GetSentinelUpdate,Result<SentinelUpdate>> |
    Query<GetSentinelFile,Result<SentinelFile>> |
    Query<GetSentinelPosition,Option<GeoPoint4>> |

    //-- messages we get from our connector
    InitializeStore |
    UpdateStore |
    ConnectorError
}

pub struct SentinelActor <C,I,U> 
    where C: SentinelConnector + Send,  I: DataRefAction<SentinelStore>,  U: DataAction<SentinelUpdate>
{
    connector: C,               // where we get the external data from
    sentinels: SentinelStore,   // our internal store

    init_action: I,             // initialized interaction (triggered by self)
    update_action: U,           // update interactions (triggered by self)
}

impl<C,I,U> SentinelActor <C,I,U>
    where C: SentinelConnector + Send, I: DataRefAction<SentinelStore>, U: DataAction<SentinelUpdate>
{
    pub fn new (connector: C, init_action: I, update_action: U)->Self {
        SentinelActor { connector, sentinels: SentinelStore::new(), init_action, update_action }
    }

    async fn init_store (&mut self, sentinels: SentinelStore)->Result<()> {
        self.sentinels = sentinels;
        self.init_action.execute(&self.sentinels).await;
        Ok(())
    }

    async fn update (&mut self, sentinel_update: SentinelUpdate)->Result<()> {
        let SentinelChange { added, removed } = self.sentinels.update_with( sentinel_update, self.connector.max_history());

        if let Some(added) = added {
            self.update_action.execute(added).await;
        }
        // TODO- shall we also notify about removed records?
        Ok(())
    }

    async fn handle_record_query (&self, record_query: Query<GetSentinelUpdate,Result<SentinelUpdate>>)->Result<()> {
        let res = match self.sentinels.get_update( &record_query.question.record_id) {
            Some(upd) => Ok(upd.clone()),
            None => Err( OdinSentinelError::NoSuchRecordError(record_query.question.record_id.clone()))
        };
        record_query.respond( res).await.map_err(|_| op_failed("receiver closed"))
    }

    async fn handle_position_query( &self, query: Query<GetSentinelPosition,Option<GeoPoint4>>)->Result<()> {
        if let Some(sentinel) = self.sentinels.get( &query.question.device_id) {
            query.respond( sentinel.get_position_at( query.question.date)).await.map_err(|_| op_failed("receiver closed"))
        } else {
            query.respond(None).await.map_err(|_| op_failed("receiver closed"))
        }
    }
}

impl_actor! { match msg for Actor< SentinelActor<C,I,U>, SentinelActorMsg> 
    where C: SentinelConnector + Send + Sync,  I: DataRefAction<SentinelStore> + Sync,  U: DataAction<SentinelUpdate> + Sync
    as  
    //--- user messages
    ExecSnapshotAction => cont! {
        msg.0.execute( &self.sentinels).await;
    }
    Query<GetSentinelUpdate,Result<SentinelUpdate>> => cont! { 
        self.handle_record_query(msg).await;
    }
    Query<GetSentinelFile,Result<SentinelFile>> => cont! { 
        self.connector.handle_sentinel_file_query(msg).await; // might be in-flight, hand over to connector
    }
    Query<GetSentinelPosition,Option<GeoPoint4>> => cont! {
        self.handle_position_query(msg).await;
    }

    //--- connector messages
    InitializeStore => cont! { 
        self.init_store( msg.0).await;
    }
    UpdateStore => cont! { 
        self.update( msg.0).await; 
    }
    ConnectorError => cont! { 
        error!("connector error: {:?}", msg) // TODO - this needs to be handled
    }

    _Start_ => cont! {
        let hself = self.hself.clone();
        if let Err(e) = self.connector.start( hself).await {  // this should eventually lead to an InitializeStore
            error!("failed to start connector: {:?}", e)
        }
    }
    _Terminate_ => stop! { 
        self.connector.terminate(); 
    }
}

