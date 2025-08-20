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

/// example of how to process long running, overlapping requests sequentially in a background task by means
/// of a [`RequestProcessor`].
/// 
/// Note that the request type arg for `RequestProcessor` does not have to be a query - it can be anything that is
/// Send + 'static and also allows the RequestProcessor impl to decide if two requests are equivalent.
/// 
/// However, we also want to be able to directly process [`Query`] requests without additional runtime
/// overhead, which is what this example does

use odin_actor::{errors::{op_failed, Result}, prelude::*};
use odin_common::datetime::{millis, secs};
use trait_set::trait_set;

/* #region common app types ******************************************************************************************/

/// the query question type
#[derive(Clone,Debug)] 
pub struct GetFile { 
    filename: String 
}
impl GetFile {
    fn new (filename: impl ToString)->Self { GetFile { filename: filename.to_string() } }
}

/// the query answer type
#[derive(Clone, Debug)]
pub struct FileAvailable {
    pathname: String
}

define_actor_msg_set! { ClientActorMsg } // we only process system messages

/* #endregion common app types */

/* #region client actors *********************************************************************************************/

trait_set! {
  trait ServerConstraints = MsgReceiver<Query<GetFile,FileAvailable>> + Send + Sync;
}

struct Actor1State<S> { server: S }
impl_actor! { match msg for Actor<Actor1State<S>,ClientActorMsg> where S: ServerConstraints as
    _Start_ => cont! { 
        let filename = "foo";
        println!("{} sending query for '{filename}'", self.id());
        match query_ref(&self.server, GetFile::new(filename)).await {
            Ok(response) => println!("{} got query response: '{}'", self.id(), response.pathname),
            Err(e) => println!("{} got query response error: {:?}", self.id(), e)
        }
    }
}

#[derive(Debug)] struct KickOffQueries{}
define_actor_msg_set! { Actor2Msg = KickOffQueries }

struct Actor2State<S> { server: S }
// this one send two queris
impl_actor! { match msg for Actor<Actor2State<S>,Actor2Msg> where S: ServerConstraints as
    _Start_ => cont!{
        self.send_msg( KickOffQueries{}).await;
    }
    KickOffQueries => term! { // once this is done we terminate
        let filename = "foo";
        println!("{} sending query for '{filename}'", self.id());
        match query_ref(&self.server, GetFile::new(filename)).await {
            Ok(response) => {
                println!("{} got query response: '{}'", self.id(), response.pathname);

                // now that we got the first query response, send a second query
                let filename = "baz";
                println!("{} sending query for '{filename}'", self.id());
                match query_ref(&self.server, GetFile::new(filename)).await {
                    Ok(response) => println!("{} got query response: '{}'", self.id(), response.pathname),
                    Err(e) => println!("{} got query response error: {:?}", self.id(), e)
                }
            }
            Err(e) => println!("{} got query response error: {:?}", self.id(), e)
        }
    }
}

struct Actor3State<S> { server: S }
impl_actor! { match msg for Actor<Actor3State<S>,ClientActorMsg> where S: ServerConstraints as
    _Start_ => cont! { 
        let filename = "bar";
        println!("{} sending query for '{filename}'", self.id());
        match query_ref(&self.server, GetFile::new(filename)).await {
            Ok(response) => println!("{} got query response: '{}'", self.id(), response.pathname),
            Err(e) => println!("{} got query response error: {:?}", self.id(), e)
        }
    }
}
/* #endregion client actors */

/* #region server actor **********************************************************************************************/

/// our sequential RequestProcessor for Query<GetFile,FileAvailable> requests
pub struct FileFetcher {}

impl RequestProcessor<Query<GetFile,FileAvailable>,FileAvailable> for FileFetcher {
    async fn get_response_future (&self, req: Option<Query<GetFile,FileAvailable>>) -> Option<(Query<GetFile,FileAvailable>,FileAvailable)> {
        if let Some(request) = req {
            let pathname = format!("/somedir/{}", request.question.filename);
            println!(".. server getting '{}'..", request.question.filename);
            sleep( secs(2)).await; // simulated get - we could model a random response time here
            println!(".. server saved '{}' to '{}'.", request.question.filename, pathname);
            Some( (request, FileAvailable { pathname }) )
        } else { None }
    }

    // just respond to the query, which should wake the respective client actor
    async fn process_response (&self, request: &Query<GetFile,FileAvailable>, answer: FileAvailable) -> Result<()> {
        request.respond(answer).await
    }

    fn is_same_request (&self, request1: &Query<GetFile,FileAvailable>, request2: &Query<GetFile,FileAvailable>)->bool {
        request1.question.filename == request2.question.filename
    }
}

define_actor_msg_set! { ServerMsg = Query<GetFile,FileAvailable> }

struct ServerState{
    request_task: AbortHandle,
    request_tx: MpscSender<Query<GetFile,FileAvailable>>
}

impl ServerState {
    fn new ()->Result<Self> {
        let (request_task, request_tx) = FileFetcher{}.spawn("file_fetcher", 16)?;
        Ok( ServerState { request_task, request_tx } )
    }
}

impl_actor! { match msg for Actor<ServerState,ServerMsg> as
    Query<GetFile,FileAvailable> => cont! {
        self.request_tx.send( msg).await; // we just hand the query over to our sequential request processsor
    }
}

/* #endregion server actor */

#[tokio::main]
async fn main ()->Result<()> {
    let mut actor_system = ActorSystem::new("main");

    let hserver = spawn_actor!( actor_system, "server", ServerState::new()?)?;
    let _hactor1 = spawn_actor!( actor_system, "actor1", Actor1State{server: hserver.clone()})?;
    let _hactor2 = spawn_actor!( actor_system, "actor2", Actor2State{server: hserver.clone()})?;
    let _hactor3 = spawn_actor!( actor_system, "actor3", Actor3State{server: hserver.clone()})?;

    actor_system.timeout_start_all(millis(20)).await?; // sends out _Start_ messages
    actor_system.process_requests().await?;

    Ok(())
}