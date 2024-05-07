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

#![allow(unused)]

use std::{fmt::Debug, future::{ready, Future, Ready}, marker::PhantomData, ops::{Deref, DerefMut, Fn}, pin::Pin, time::Duration};
use paste::paste;
use tracing_subscriber::registry::Data;
use crate::{DynMsgReceiver, MsgReceiver,errors::{Result, OdinActorError}};


/* #region MsgSubscriber *******************************************************************************/

/// MsgSubscriber is the pattern to use if the publisher is defining the message to send out. While
/// this hides the type of the receiver it still requires all the receivers to be partly homogenous
/// (processing the same message). 
/// This is a trait object safe subscriber for receiving messages of type `M`, which
/// are created by the actor we subscribe to. While this has a low runtime overhead
/// it requires the subscribers to be homogenous (all receiving the same message type `M`),
/// i.e. it exposes subscriber details to the actor we subscribe to and hence reduces
/// re-usability. Use the more abstract `ActionSubscriptions` if this is not suitable
pub type MsgSubscriber<M> = Box<dyn DynMsgReceiver<M> + Send + Sync + 'static>;

pub fn msg_subscriber<M> (s: impl DynMsgReceiver<M> + Send + Sync + 'static)->MsgSubscriber<M> {
    Box::new(s)
}

/// container to keep a dynamically updated list of homogenous DynMsgReceiver instances.
/// MsgSubscriptions objects are used as fields within the actor we subscribe to, to implement a
/// publish/subscribe pattern that hides the concrete types of the subscribers (which don't even have to be actors) 
pub struct MsgSubscriptions<M>
    where M: Send + Clone + Debug + 'static
{
    list: Vec<MsgSubscriber<M>>, 
}

// TODO - should we automatically remove subscribers we fail to send to?
impl<M> MsgSubscriptions<M> 
    where M: Send + Clone + Debug + 'static
{
    pub fn new()->Self {
        MsgSubscriptions { list: Vec::new() }
    }

    pub fn add (&mut self, subscriber: MsgSubscriber<M>) {
        self.list.push( subscriber);
    }

    pub async fn publish_msg (&self, msg: M) -> Result<()> {
        for p in &self.list {
            p.send_msg( msg.clone()).await?;
        }
        Ok(())
    }

    pub async fn timeout_publish_msg (&self, msg: M, to: Duration) -> Result<()> {
        for ref p in &self.list {
            p.timeout_send_msg( msg.clone(), to).await?;
        }
        Ok(())
    }
}

/* #endregion MsgSubscriber */

/* region message actions ********************************************************************************/

/// an actor action that sends a message to fixed set of actors.
/// This is basically a list of ActorHandles implementing the same MsgReceiver<T> that we async send the same message T.
/// Use this if the action owner is in control of what message to send.
/// To create an ActorMsgList use the define_actor_msg_list!() macro
pub trait MsgAction<T>: Send where T: Clone + Debug + Send {
    fn execute (&self,m: T) -> impl Future<Output=Result<()>> + Send;
} 


/* end region message actions */

// Data[Ref]Actions have been generalized and moved to odin_action (which is automatically re-exported by odn_actor)