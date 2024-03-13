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
use odin_actor::prelude::*;
use odin_actor::tokio_kanal::{ActorSystem,ActorSystemHandle,Actor,ActorHandle,Query,
    AbortHandle,JoinHandle, MpscSender,MpscReceiver,create_mpsc_sender_receiver,spawn
};
use crate::*;
use crate::ws::WsCmd;

//-- external messages
#[derive(Debug)] pub struct AddInitCallback { pub id: String, pub action: Callback<&SentinelStore> }
#[derive(Debug)] pub struct AddUpdateCallback { pub id: String, pub action: Callback<SentinelUpdate> }
#[derive(Debug)] pub struct GetSentinelUpdate { pub record_id: String }
#[derive(Debug)] pub struct GetSentinelFile { pub record_id: String, pub filename: String }
#[derive(Debug)] pub struct SendSentinelCmd { sentinel_cmd: WsCmd }

//-- internal messages. Note these are not public since we should only get them from our connector
#[derive(Debug)] struct InitializeStore (SentinelStore);
#[derive(Debug)] struct UpdateStore (SentinelUpdate);
#[derive(Debug)] struct ConnectorError (OdinSentinelError);

define_actor_msg_type! { pub SentinelActorMsg = 
    //-- messages we get from other actors
    AddInitCallback |
    AddUpdateCallback |
    Query<GetSentinelUpdate,Option<SentinelUpdate>> |
    Query<GetSentinelFile,Option<PathBuf>> |

    //-- messages we get from our connector
    InitializeStore |
    UpdateStore |
    ConnectorError
}

define_struct! { pub SentinelActor<S> where S: SentinelConnector = 
    connector: S,

    sentinels: SentinelStore = SentinelStore::new(),
    
    init_callbacks: CallbackList<&SentinelStore> = CallbackList::new(),
    update_callbacks: CallbackList<SentinelUpdate> = CallbackList::new()
}

impl<S> SentinelActor<S> where S: SentinelConnector + Send {
    async fn init_store (&mut self, sentinel_store: SentinelStore)->Result<()> {
        self.sentinels = sentinel_store;
        self.request_all_image_files().await?;

        self.init_callbacks.trigger(&self.sentinels).await;
        Ok(())
    }

    async fn update (&mut self, sentinel_update: SentinelUpdate)->Result<()> {
        let SentinelChange { added, removed } = self.sentinels.update_with( sentinel_update, self.connector.max_history());

        if let Some(ref added) = added {
            match_algebraic_type! { added: SentinelUpdate as
                Arc<SensorRecord<ImageData>> => { self.connector.request_image_file(added).await; }
                _ => {}
            }
            self.update_callbacks.trigger(added.clone()).await;
        }
        // TODO- shall we also notify about removed records?
        Ok(())
    }

    async fn handle_record_query (&self, record_query: Query<GetSentinelUpdate,Option<SentinelUpdate>>) {
        let resp = self.sentinels.get_update( &record_query.question.record_id).map(|u| u.clone());
        record_query.respond( resp).await;
    }

    async fn request_all_image_files (&self)->Result<()> {
        for sentinel in self.sentinels.values_iter() {
            for rec in &sentinel.image {
                self.connector.request_image_file( rec).await?;
            }
        }
        Ok(())
    }
}

impl_actor! { match msg for Actor<SentinelActor<S>,SentinelActorMsg> where S: SentinelConnector + Send as 
    _Start_ => cont! { self.connector.start( self.hself.clone()).await; }

    //--- user messages
    AddInitCallback => cont! { self.init_callbacks.add( msg.id, msg.action ) }
    AddUpdateCallback => cont! { self.update_callbacks.add( msg.id, msg.action ) }
    Query<GetSentinelUpdate,Option<SentinelUpdate>> => cont! { self.handle_record_query(msg).await; }
    Query<GetSentinelFile,Option<PathBuf>> => cont! { 
        // self.connector.handle_file_query(msg).await; 
    }

    //--- connector messages
    InitializeStore => cont! { self.init_store( msg.0).await; }
    UpdateStore => cont! { self.update( msg.0).await; }

    _Terminate_ => stop! { self.connector.terminate(); }
}

/// this is the abstraction over the actual source of external information, which can be either a live connection to a Delphire server
/// or a replayer for archived data
pub trait SentinelConnector {
   async fn start (&mut self, hself: ActorHandle<SentinelActorMsg>)->Result<()>; // ?? does this have to be async?
   async fn send_cmd (&mut self, cmd: WsCmd)->Result<()>;

   async fn request_image_file (&self, rec: &SensorRecord<ImageData>)->Result<()>;
   async fn handle_file_query (&self, file_query: Query<SentinelFile,Option<PathBuf>>);
   fn terminate (&mut self);

   fn max_history(&self)->usize;

}
