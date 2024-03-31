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

#![allow(unused,private_interfaces)]
#![feature(trait_alias)]

use odin_actor::prelude::*;
use odin_actor::{error,debug,warn,info};
use crate::*;
use crate::ws::WsCmd;

//-- external messages (from other actors)

/// request a specific update record
#[derive(Debug)] pub struct GetSentinelUpdate { pub record_id: String }

/// request a file associated with an update record
#[derive(Debug)] pub struct GetSentinelFile { pub record_id: String, pub filename: String }

#[derive(Debug)] pub struct ExecSnapshotAction(String);

/// send a command to Sentinel devices
#[derive(Debug)] pub struct SendSentinelCmd { sentinel_cmd: WsCmd }

//-- internal messages. Note these are not public since we should only get them from our connector
#[derive(Debug)] pub(crate) struct InitializeStore (pub(crate) SentinelStore);
#[derive(Debug)] pub(crate) struct UpdateStore (pub(crate) SentinelUpdate);
#[derive(Debug)] pub(crate) struct ConnectorError (pub(crate) OdinSentinelError);

define_actor_msg_type! { pub SentinelActorMsg = 
    //-- messages we get from other actors
    ExecSnapshotAction |
    Query<GetSentinelUpdate,Option<SentinelUpdate>> |
    Query<GetSentinelFile,Option<PathBuf>> |

    //-- messages we get from our connector
    InitializeStore |
    UpdateStore |
    ConnectorError
}

pub trait InitAction = ActorAction<SentinelStore>;
pub trait UpdateAction = ActorAction<SentinelUpdate>;
pub trait SnapshotAction = ActorAction2<SentinelStore,String>;

pub struct SentinelActor<S,A,B,C> 
    where S: SentinelConnector + Send, A: InitAction, B: UpdateAction, C: SnapshotAction
{
    connector: S,             // where we get the external data from
    sentinels: SentinelStore, // our internal store

    init_action: A,           // initialized interaction (triggered by self)
    update_action: B,         // update interactions (triggered by self)
    snapshot_action: C,       // on-demand interactions based on the whole store
}

impl<S,A,B,C> SentinelActor<S,A,B,C> 
    where S: SentinelConnector + Send, A: InitAction, B: UpdateAction, C: SnapshotAction
{
    pub fn new (connector: S, init_action: A, update_action: B, snapshot_action: C)->Self {
        SentinelActor { connector, sentinels: SentinelStore::new(), init_action, update_action, snapshot_action }
    }

    async fn init_store (&mut self, sentinels: SentinelStore)->Result<()> {
        self.sentinels = sentinels;
        self.request_all_image_files().await;
        self.init_action.execute(&self.sentinels).await;
        Ok(())
    }

    async fn request_all_image_files (&self)->Result<()> {
        for sentinel in self.sentinels.values_iter() {
            for rec in &sentinel.image {
                self.connector.request_image_file( rec).await?;
            }
        }
        Ok(())
    }

    async fn update (&mut self, sentinel_update: SentinelUpdate)->Result<()> {
        let SentinelChange { added, removed } = self.sentinels.update_with( sentinel_update, self.connector.max_history());

        if let Some(ref added) = added {
            match_algebraic_type! { added: SentinelUpdate as
                Arc<SensorRecord<ImageData>> => { self.connector.request_image_file(added).await; }
                _ => {}
            }
            self.update_action.execute(added).await;
        }
        // TODO- shall we also notify about removed records?
        Ok(())
    }

    async fn handle_record_query (&self, record_query: Query<GetSentinelUpdate,Option<SentinelUpdate>>)->Result<()> {
        let resp = self.sentinels.get_update( &record_query.question.record_id).map(|u| u.clone());
        record_query.respond( resp).await.map_err(|_| op_failed("receiver closed"))
    }


}

impl_actor! { match msg for Actor<SentinelActor<S,A,B,C>,SentinelActorMsg> 
    where S: SentinelConnector + Send + Sync, A: InitAction + Sync, B: UpdateAction + Sync, C: SnapshotAction + Sync
    as  
    //--- user messages
    ExecSnapshotAction => cont! {
        self.snapshot_action.execute( &self.sentinels, &msg.0).await;
    }
    Query<GetSentinelUpdate,Option<SentinelUpdate>> => cont! { 
        self.handle_record_query(msg).await; 
    }
    Query<GetSentinelFile,Option<PathBuf>> => cont! { 
        self.connector.handle_file_query(msg).await; 
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
            error!("failed to start connector")
        }
    }
    _Terminate_ => stop! { 
        self.connector.terminate(); 
    }
}

/// this is the abstraction over the actual source of external information, which can be either a live connection to a Delphire server
/// or a replayer for archived data
pub trait SentinelConnector {

   fn start (&mut self, hself: ActorHandle<SentinelActorMsg>) -> impl Future<Output=Result<()>> + Send;

   fn send_cmd (&mut self, cmd: WsCmd) -> impl Future<Output=Result<()>> + Send;
   fn request_image_file (&self, rec: &SensorRecord<ImageData>) -> impl Future<Output=Result<()>> + Send;
   fn handle_file_query (&self, file_query: Query<GetSentinelFile,Option<PathBuf>>) -> impl Future<Output=Result<()>> + Send;

   fn terminate (&mut self);

   fn max_history(&self)->usize;
}
