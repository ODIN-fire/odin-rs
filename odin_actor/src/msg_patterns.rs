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

use std::{fmt::Debug, future::{ready, Future, Ready}, marker::PhantomData, ops::Fn, pin::Pin, time::Duration};
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

/* region data actions ***********************************************************************************/

/// an action where the sender does not need to know which messages to send to whom - it only provides
/// the data that can be used to construct messages, which do *not* have to be homogenous (like in MsgAction)
/// This action type consumes the data passed into `execute(T)` and therefore should be used for data that
/// is generated for each `DataAction` execution. Use [`DataRefAction`] and the accompanying [`dataref_action`]
/// macro if the data should be borrowed from the sender 
pub trait DataAction<T>: Send + Sync {
    fn execute (&self, data: T) -> impl std::future::Future<Output = Result<()>> + Send;
}

/// a macro that both defines and then instantiates a transparent DataAction type, with the general form
/// ```
/// data_action!( «captured-receiver-var» as MsgReceiver<M>, .. => |«data-var» : «data-var-type»| «execute-expr»)
/// ```
/// which expands
/// ```
///    let recvA = spawn_actor!(...)?;
///    ...
///    data_action!( recvA as MsgReciever<Msg1> => |data: MyData| {
///       recvA.send_msg( Msg1::new(.. data ..)).await;
///       ...
///    })
/// ``` 
/// into something like
/// ```
/// struct SomeAction<TA:MsgReceiver<Msg1>,TB:MsgReceiver<Msg2>,..>( recvA: TA, recvB: TB, ...);
/// impl <TA,TB,..> DataAction<T> for SomeAction<TA,TB,..> { 
///    async fn execute (&self, data: MyData)->Result<()> {
///       let recvA = self.recvA.clone();
///       { .. recvA.send_msg( Msg1::new(... data ...)).await .. }
///    }
/// }
/// ```
/// This acts like a closure that captures actor handles (`recvA`) from its call site but does not use a Fn(..)
/// to do so. This allows to execute the action without pin-boxing the execute(..) future, i.e. does *not* incur runtime
/// cost when executing
#[macro_export]
macro_rules! data_action {
    ( $( $recv:ident $(. $op:ident ())? as $(MsgReceiver< $msg_t:ty >)|* ),* => |$data:ident : $data_type:ty| $e:expr ) => {
        paste::paste! {
            {
                struct SomeAction< $( [<T $recv>]: $( MsgReceiver<$msg_t> + )* Sized ),* > { $( $recv: [<T $recv>] ),* }

                impl< $( [<T $recv>] ),* > DataAction<$data_type> for SomeAction < $( [<T $recv>] ),* >
                    where $( [<T $recv>]: $( MsgReceiver<$msg_t> + )* Sized ),* 
                {
                    async fn execute (&self, $data: $data_type)->std::result::Result<(),OdinActorError> {
                        $( let $recv = &self.$recv; )*
                        $e
                    }
                }

                $( let $recv = $recv $(. $op () )? ; )*
                SomeAction{ $( $recv ),* }
            }
        }
    }
}

pub struct NoDataAction<T: Send + Sync> { _phantom: PhantomData<T> }
impl <T: Send + Sync> NoDataAction<T> { 
    pub fn new ()->Self { NoDataAction { _phantom: PhantomData } }
}
impl<T: Send + Sync> DataAction<T> for NoDataAction<T> {
    async fn execute (&self, data: T)->Result<()> { Ok(()) }
}



/// a `DataAction` that is executed with a reference to the sender's data. Use this if the data used in execute(&T) is
/// directly stored as a field of the sender
pub trait DataRefAction<T>: Send + Sync {
    fn execute (&self, data: &T) -> impl std::future::Future<Output = Result<()>> + Send;
}

/// the corresponding macro to define and create a [`DataRefAction<T>`], similar to [`data_action!`]
#[macro_export]
macro_rules! dataref_action {
    ( $( $recv:ident $(. $op:ident ())? as $(MsgReceiver< $msg_t:ty >)|* ),* => |$data:ident : & $data_type:ty| $e:expr ) => {
        paste::paste! {
            {
                struct SomeAction< $( [<T $recv>]: $( MsgReceiver<$msg_t> + )* Sized ),* > { $( $recv: [<T $recv>] ),* }

                impl< $( [<T $recv>] ),* > DataRefAction<$data_type> for SomeAction < $( [<T $recv>] ),* >
                    where $( [<T $recv>]: $( MsgReceiver<$msg_t> + )* Sized ),*
                {
                    async fn execute (&self, $data: & $data_type)->std::result::Result<(),OdinActorError> {
                        $( let $recv = &self.$recv; )*
                        $e
                    }
                }

                $( let $recv = $recv $(. $op () )? ; )*
                SomeAction{ $( $recv ),* }
            }
        }
    }
}

pub struct NoDataRefAction<T: Send + Sync> { _phantom: PhantomData<T> }
impl <T: Send + Sync> NoDataRefAction<T> { 
    pub fn new ()->Self { NoDataRefAction { _phantom: PhantomData } }
}
impl<T: Send + Sync> DataRefAction<T> for NoDataRefAction<T> {
    async fn execute (&self, data: &T)->Result<()> { Ok(()) }
}


/// a [`DataAction`] that can be labeled, e.g. to preserve some information from an incoming request message (triggering this
/// action) that should be passed along together with the sender data
pub trait LabeledDataAction<A,B>: Send + Sync {
    fn execute (&self, label: A, data: B) -> impl std::future::Future<Output = Result<()>> + Send;
}

/// macro to define and create a [`LabeledDataAction`]. See [`data_action!`]
#[macro_export]
macro_rules! labeled_data_action {
    ( $( $recv:ident $(. $op:ident ())? as $(MsgReceiver< $msg_t:ty >)|* ),* => |$label:ident : $label_type:ty, $data:ident : $data_type:ty| $e:expr ) => {
        paste::paste! {
            {
                struct SomeAction< $( [<T $recv>]: $( MsgReceiver<$msg_t> + )* Sized ),* > { $( $recv: [<T $recv>] ),* }

                impl< $( [<T $recv>] ),* > LabeledDataAction<$label_type,$data_type> for SomeAction < $( [<T $recv>] ),* >
                    where $( [<T $recv>]: $( MsgReceiver<$msg_t> + )* Sized ),*
                {
                    async fn execute (&self, $label: $label_type, $data: $data_type)->std::result::Result<(),OdinActorError> {
                        $( let $recv = &self.$recv; )*
                        $e
                    }
                }

                $( let $recv = $recv $(. $op () )? ; )*
                SomeAction{ $( $recv ),* }
            }
        }
    }
}

pub struct NoLabeledDataAction<A: Send+Sync, B: Send+Sync> { _phantom_a: PhantomData<A>, _phantom_b: PhantomData<B> }
impl<A: Send+Sync, B: Send+Sync> NoLabeledDataAction<A,B> { 
    pub fn new ()->Self { NoLabeledDataAction { _phantom_a: PhantomData, _phantom_b: PhantomData } }
}
impl<A: Send+Sync, B: Send+Sync> LabeledDataAction<A,B> for NoLabeledDataAction<A,B> {
    async fn execute (&self, label: A, data: B)->Result<()> { Ok(()) }
}

/// similar to [`LabeledDataAction`], except that it uses a reference of the sender data when calling `execute(label,&data)` (see also [`DataRefAction`])
pub trait LabeledDataRefAction<A,B>: Send + Sync {
    fn execute (&self, label: A, data: &B) -> impl std::future::Future<Output = Result<()>> + Send;
}

/// macro to define and create a [`LabeledDataRefAction`]. See [`data_action!`]
#[macro_export]
macro_rules! labeled_dataref_action {
    ( $( $recv:ident $(. $op:ident ())? as $(MsgReceiver< $msg_t:ty >)|* ),* => |$label:ident : $label_type:ty, $data:ident : & $data_type:ty| $e:expr ) => {
        paste::paste! {
            {
                struct SomeAction< $( [<T $recv>]: $( MsgReceiver<$msg_t> + )* Sized ),* > { $( $recv: [<T $recv>] ),* }

                impl< $( [<T $recv>] ),* > LabeledDataRefAction<$label_type,$data_type> for SomeAction < $( [<T $recv>] ),* >
                    where $( [<T $recv>]: $( MsgReceiver<$msg_t> + )* Sized ),*
                {
                    async fn execute (&self, $label: $label_type, $data: & $data_type)->std::result::Result<(),OdinActorError> {
                        $( let $recv = &self.$recv; )*
                        $e
                    }
                }

                $( let $recv = $recv $(. $op () )? ; )*
                SomeAction{ $( $recv ),* }
            }
        }
    }
}

pub struct NoLabeledDataRefAction<A: Send+Sync, B: Send+Sync> { _phantom_a: PhantomData<A>, _phantom_b: PhantomData<B> }
impl<A: Send+Sync, B: Send+Sync> NoLabeledDataRefAction<A,B> { 
    pub fn new ()->Self { NoLabeledDataRefAction { _phantom_a: PhantomData, _phantom_b: PhantomData } }
}
impl<A: Send+Sync, B: Send+Sync> LabeledDataRefAction<A,B> for NoLabeledDataRefAction<A,B> {
    async fn execute (&self, label: A, data: &B)->Result<()> { Ok(()) }
}

/* endregion data actions */

/* #region dyn actions ***********************************************************************************/

/// a DataAction that can be dynamically created and sent in a message
/// this incurs significant runtime cost for async actions compared to normal DataActions 
/// (we need to box the DynDataAction, which then needs to pin/box the execute functions). We therefore
/// distinguish between AsyncDynDataAction and the slightly less SyncDynDataAction (which avoids the
/// per-execution pin/box)
pub enum DynDataAction<T> {
    Sync(SyncDynDataAction<T>),
    Async(AsyncDynDataAction<T>),
}

impl<T> DynDataAction<T> {
    pub async fn execute (&self, v: &T)->Result<()> {
        match self {
            DynDataAction::Sync(dda) => (dda.action)(v),
            DynDataAction::Async(dda) => (dda.action)(v).await,
        }
    }
}

impl <T> From<AsyncDynDataAction<T>> for DynDataAction<T> {
    fn from (dda: AsyncDynDataAction<T>)->Self { DynDataAction::Async(dda) }
}
impl <T> From<SyncDynDataAction<T>> for DynDataAction<T> {
    fn from (dda: SyncDynDataAction<T>)->Self { DynDataAction::Sync(dda) }
}

impl<T> Debug for DynDataAction<T> {
    fn fmt (&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DynDataAction::Sync(dda) => write!(f, "SyncDynDataAction<{}>(..)", std::any::type_name::<T>()),
            DynDataAction::Async(dda) => write!(f, "AsyncDynDataAction<{}>(..)", std::any::type_name::<T>())
        }
    }
}

//--- async DynDataAction (send_msg, try_sent_msg)

pub type AsyncDynDataActionFuture = Pin<Box<dyn Future<Output = Result<()>> + Send>>;
pub trait AsyncDynDataActionFn<T> = (Fn(T)->AsyncDynDataActionFuture) + Send + Sync;

pub struct AsyncDynDataAction<T> {
    pub action: Box<dyn for<'a> AsyncDynDataActionFn<&'a T>>
}

impl <T> AsyncDynDataAction<T> {
    pub async fn execute (&self, data: &T)->Result<()> {
        (&self.action)(data).await
    }
}

macro_rules! async_action {
    ($b: block) => { Box::pin( async move $b ) };
    ($e: expr) => { Box::pin( async move { $e }) };
}

#[macro_export]
macro_rules! async_dyn_data_action {
    ( |$v:ident $(: $t:ty)?| $b:block ) => {
        AsyncDynDataAction { action: Box::new( move |$v $(: $t )?| $b ) }
    };
    ( |$v:ident $(: $t:ty)?| $e:expr ) => {
        AsyncDynDataAction { action: Box::new( move |$v $(: $t )?| { Box::pin( async move { $e }) } ) }
    };
}

/// a specialized AsyncDynDataAction that creates and sends a message constructuted from
/// the execute(data) argument. This is worth a macro since we need to clone the receiver
/// twice (when capturing it in the DynDataAction and then again when pinboxing the action)
#[macro_export]
macro_rules! send_msg_dyn_action {
    ( $rcv:expr, |$v:ident $(: $t:ty)?| $e:expr ) => {
        {
            let rcv = ($rcv).clone();
            DynDataAction::Async(
                AsyncDynDataAction{ action: 
                    Box::new( move |$v $(: $t)?| {
                        let msg = $e; 
                        let rcv = rcv.clone();
                        { Box::pin( async move { rcv.send_msg( msg).await } ) }
                    })
                }
            )
        }
    }
}

//--- sync DynDataAction (try_send_msg, logging etc.)

/// a DynDataAction that is sync and therefore does not need to pinbox an action future
pub trait SyncDynDataActionFn<T> = (Fn(T)->Result<()>) + Send + Sync;

pub struct SyncDynDataAction<T>{
    pub action: Box<dyn for<'a> SyncDynDataActionFn<&'a T>>
}

#[macro_export]
macro_rules! sync_dyn_data_action {
    ( |$v:ident $(: $t:ty)?| $b:block ) => {
        SyncDynDataAction{ action: Box::new( move |$v $(: $t )?| $b ) }
    };
    ( |$v:ident $(: $t:ty)?| $e:expr ) => {
        SyncDynDataAction{ action: Box::new( move |$v $(: $t )?| { $e } ) }
    };
}

#[macro_export]
macro_rules! try_send_msg_dyn_action {
    ( $rcv:expr, |$v:ident $(: $t:ty)?| $e:expr) => {
        {
            let rcv = $rcv.clone();
            DynDataAction::Sync(
                SyncDynDataAction{ action: 
                    Box::new( move |$v $(: $t)?| {
                        let msg = $e; 
                        rcv.try_send_msg( msg)
                    })
                }
            )
        }
    }
}


/// the field to store actions in. Note that we need to store trait objects here so that the owner does not
/// have to know action specifics, only its own associated input data type T 
pub struct DynDataActionList<T> { 
    entries: Vec<DynDataAction<T>> 
}

impl <T> DynDataActionList<T> {
    pub fn new()->Self { 
        DynDataActionList{ entries: Vec::new() } 
    }

    pub fn is_empty (&self)->bool {
        self.entries.is_empty()
    }
    
    pub fn push (&mut self, cb: DynDataAction<T>) { 
        self.entries.push( cb)
    }

    pub async fn execute (&self, v: &T)->Result<()> {
        let mut failed = 0;

        for cb in &self.entries {
            let res: Result<()> = match cb {
                DynDataAction::Async(cb) => (cb.action)(v).await,
                DynDataAction::Sync(cb) => (cb.action)(v),
            };

            if res.is_err() { failed += 1 }
        }

        if failed > 0 {
            Err( OdinActorError::IterOpFailed { op: "callback execution".to_string(), all: self.entries.len(), failed })
        } else {
            Ok(())
        }
    }
}

/* #endregion callbacks */
