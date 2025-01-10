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

/// message object to indicate a device hasn't reported within a configured amount of time
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct SentinelInactiveAlert {
    pub device_id: String,
    pub last_time_recorded: Option<DateTime<Utc>>,
}

const INACTIVE_TIMER: i64 = 1;

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

pub struct SentinelActor <C,I,U,IA> 
    where C: SentinelConnector + Send,  I: DataRefAction<SentinelStore>,  U: DataAction<SentinelUpdate>, IA: DataAction<SentinelInactiveAlert>
{
    connector: C,               // where we get the external data from
    sentinels: SentinelStore,   // our internal store

    init_action: I,             // initialized interaction (triggered by self)
    update_action: U,           // update interactions (triggered by self)
    inactive_action: IA,        // inactive device alert interactions
}

impl<C,I,U,IA> SentinelActor <C,I,U,IA>
    where C: SentinelConnector + Send, I: DataRefAction<SentinelStore>, U: DataAction<SentinelUpdate>, IA: DataAction<SentinelInactiveAlert>
{
    pub fn new (connector: C, init_action: I, update_action: U, inactive_action: IA)->Self {
        SentinelActor { connector, sentinels: SentinelStore::new(), init_action, update_action, inactive_action }
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

    // note this is a server-side inactive check, i.e. new client connections won't see a status change until the
    // next server check runs. If we want this instantly we should transmit the inactive_duration through the websocket
    // during the init_action and then perform the check when receiving the sentinels on the client. Alternatively the SentinelService 
    // could send another ExecSnapshotAction to the sentinel actor that triggers a server side check with the appropriate distribution
    // (broadcast/send ws msg). This seems too expensive given that we frequently perform the status check anyways
    async fn check_inactive (&self)->Result<()> {
        let now = Utc::now();
        let inactive_duration = self.connector.inactive_duration();
        for sentinel in self.sentinels.values_iter() {
            if sentinel.time_recorded.is_none() || (duration_since( &now, &sentinel.time_recorded.unwrap()) > inactive_duration) {
                let alert = SentinelInactiveAlert { 
                    device_id: sentinel.device_id.clone(), 
                    last_time_recorded: sentinel.time_recorded
                };
                self.inactive_action.execute( alert).await.map_err(|e| op_failed(e))?
            }
        }
        Ok(())
    }

}

impl_actor! { match msg for Actor< SentinelActor<C,I,U,IA>, SentinelActorMsg> 
    where C: SentinelConnector + Send + Sync,  I: DataRefAction<SentinelStore> + Sync,  
          U: DataAction<SentinelUpdate> + Sync, IA: DataAction<SentinelInactiveAlert> + Sync
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
        if let Err(e) = self.start_repeat_timer( INACTIVE_TIMER, self.connector.inactive_interval(), false) {
            error!("failed to start inactive timer")
        } 
    }
    _Timer_ => cont! {
        if msg.id == INACTIVE_TIMER {
            self.check_inactive().await;
        }
    }
    _Terminate_ => stop! { 
        self.connector.terminate(); 
    }
}

