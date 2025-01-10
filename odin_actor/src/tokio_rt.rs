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

//! the Tokio runtime specific parts of odin_actor
//! Note this is further parameterized by the respective MPSC channel implementation to use 
//! (currently [`kanal`](https://docs.rs/kanal/latest/kanal/) or [`flume`](https://docs.rs/flume/latest/flume/))
//! as specified by the `tokio_kanal` or `tokio_flume` features, which are mutually exclusive.

#![allow(unused)]
#![feature(trait_alias)]

// re-export for our macro impls
pub extern crate tokio;

use odin_job::{JobHandle, JobScheduler};
use tokio::{
    time::{self,Interval,interval},
    task::{self, JoinSet, LocalSet},
    runtime::Handle
};
use std::{
    any::type_name, boxed::Box, cell::Cell, fmt::Debug, future::Future, marker::{PhantomData, Sync}, 
    ops::{Deref,DerefMut}, pin::Pin, 
    collections::VecDeque,
    sync::{atomic::{AtomicU64, Ordering}, Arc, LockResult, Mutex, MutexGuard}, time::{Duration, Instant}
};
use futures::TryFutureExt;
use crate::{
    create_sfc, debug, error, errors::{iter_op_result, op_failed, poisoned_lock, OdinActorError, Result}, info, micros, millis, nanos, secs, trace, unpack_ping_response, warn, ActorReceiver, ActorSystemRequest, DefaultReceiveAction, DynMsgReceiver, DynMsgReceiverTrait, FromSysMsg, Identifiable, MsgReceiver, MsgReceiverConstraints, MsgSendFuture, MsgTypeConstraints, ObjSafeFuture, ReceiveAction, SendableFutureCreator, SysMsgReceiver, TryMsgReceiver, _Exec_, _Pause_, _Ping_, _Resume_, _Start_, _Terminate_, _Timer_
};
use odin_macro::fn_mut;
use odin_common::process;

/* #region channel abstractions ********************************************************************************/
/*
 * the channel section abstracts the concrete channel type we use for mpsc channels as there are many choices
 * (tokio::sync::mpsc, flume, kanal, crossbeam etc.) and they all differ to some degree. We could
 * unify with our own traits for Sender and Receiver but this wouldn't save code and would make it harder to
 * enforce we consistently use one implementation throughout our concrete runtime module. Since we already
 * apply a similar scheme for the async runtime itself we just go with functions and a match_try_send macro.
 * Note that the tokio_xx features are mutually exclusive
 */

#[cfg(feature="tokio_kanal")]
include!("kanal_channel.rs");

#[cfg(feature="tokio_flume")]
include!("flume_channel.rs");

/* #endregion channel abstractions */

/* #region runtime abstractions ********************************************************************************/
/*
 * This section is (mostly) for type and function aliases that allow us to program our own structs/traits/impls
 * without having to explicitly use runtime or channel crate specifics. While this means we still have
 * runtime/channel specific Actors, ActorHandles etc. their source code is (mostly) similar. 
 * Trying to hoist our actor constructs to crate level would require generic types that make code less readable
 * and still result in more runtime overhead (boxing/unboxing trait objects etc.). Moreover, it is not even
 * desirable to hoist some constructs since they are not compatible between runtime/channel implementations.
 */

pub type AbortHandle = task::AbortHandle;
pub type JoinHandle<T> = task::JoinHandle<T>;


#[inline]
pub async fn sleep (dur: Duration) {
    time::sleep(dur).await;
}

#[inline]
pub async fn timeout<F,R,E> (to: Duration, fut: F)->Result<R> where F: Future<Output= std::result::Result<R,E>> {
    match time::timeout( to, fut).await {
        Ok(result) => result.map_err(|_| OdinActorError::SendersDropped),
        Err(e) => Err(OdinActorError::Timeout(to))
    }
}

#[inline]
pub async fn yield_now () {
    task::yield_now().await;
}

#[inline]
pub fn spawn<F>(name: &str, future: F) -> Result<JoinHandle<F::Output>>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
{
    Ok(task::Builder::new()
        .name(name)
        .spawn(future)?)
}

#[inline]
pub fn spawn_blocking<F,R> (name: &str, fn_once: F) -> Result<JoinHandle<F::Output>>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static
{
    Ok(task::Builder::new()
        .name(name)
        .spawn_blocking( fn_once)?)
}

// these functions can be used to communicate back to the actor once the spawn_blocking() executed FnOnce is done

pub fn block_on<F: Future>(future: F) -> F::Output {
    Handle::current().block_on( future)
}

/// a specialized version that uses a try_send_msg() from within a blocking loop.
/// Note this comes with the additional cost/constraint of a Clone constraint for the message
pub fn block_on_send_msg<Msg> (tgt: impl MsgReceiver<Msg>, msg: Msg)->Result<()> where Msg: Send + Clone {
    loop {
        match tgt.try_send_msg(msg.clone()) {
            Ok(()) => return Ok(()),
            Err(e) => match e {
                OdinActorError::ReceiverFull => std::thread::sleep(millis(30)),
                _ => return Err(e)
            }
        }
    }
}

/// a timeout version of a blocking try_send_msg() loop. Use this if it is not at the end of the spawn_blocking() task
pub fn block_on_timeout_send_msg<Msg> (tgt: impl MsgReceiver<Msg>, msg: Msg, to: Duration)->Result<()> where Msg: Send + Clone {
    let mut elapsed = millis(0);
    loop {
        match tgt.try_send_msg(msg.clone()) {
            Ok(()) => return Ok(()),
            Err(e) => match e {
                OdinActorError::ReceiverFull => {
                    if elapsed > to {
                        return Err(OdinActorError::Timeout(to))
                    }
                    let dt = millis(30);
                    std::thread::sleep(dt); // note this is just an approximation but we don't try to minimize latency here
                    elapsed += dt;
                }
                _ => return Err(e)
            }
        }
    }
}

/* #endregion runtime abstractions */

/* #region Actor and ActorHandle *******************************************************************************/
/*
 * We could hoist Actor and ActorHandle if we put MpscSender and Abortable behind traits and add them as
 * generic type params but that would (a) obfuscate the code and (b) loose the capability to store hself and hsys.
 *  
 * The real optimization we would like is to avoid MsgReceiver trait objects but those seem necessary for dynamic (msg based) subscription 
 */

/// S represents the actor state type, M the message type (enum)
pub struct Actor <S,M> where S: Send + 'static, M: MsgTypeConstraints {
    pub state: S,
    pub hself: ActorHandle<M>,
}

impl <S,M> Actor <S,M> where S: Send + 'static, M: MsgTypeConstraints {
    //--- unfortunately we can only have one Deref so we forward these explicitly

    #[inline(always)]
    pub fn id (&self)->&str {
        self.hself.id()
    }

    pub fn hself (&self)->ActorHandle<M> {
        self.hself.clone()
    }

    pub fn hsys (&self)->&ActorSystemHandle {
        self.hself.hsys()
    }

    #[inline(always)]
    pub fn send_msg<T> (&self, msg: T)->impl Future<Output=Result<()>> + '_  where T: Into<M> {
        self.hself.send_actor_msg( msg.into())
    }

    #[inline(always)]
    pub fn timeout_send_msg<T> (&self, msg: T, to: Duration)->impl Future<Output=Result<()>> + '_  where T: Into<M> {
        self.hself.timeout_send_actor_msg( msg.into(), to)
    }

    #[inline(always)]
    pub fn try_send_msg<T> (&self, msg:T)->Result<()> where T: Into<M> {
        self.hself.try_send_actor_msg(msg.into())
    }

    #[inline(always)]
    pub fn get_scheduler (&self)->LockResult<MutexGuard<'_,JobScheduler>> {
        self.hsys().get_scheduler()
    }

    #[inline(always)]
    pub fn start_oneshot_timer (&self, id: i64, delay: Duration) -> Result<AbortHandle> {
        oneshot_timer_for( self.hself.clone(), id, delay)
    }

    /// each loop first waits for timer_interval to expire and then send a _Timer_ system message
    #[inline(always)]
    pub fn start_repeat_timer (&self, id: i64, timer_interval: Duration, instantly: bool) -> Result<AbortHandle> {
        repeat_timer_for( self.hself.clone(), id, timer_interval, instantly)
    }

    #[inline(always)]
    pub async fn request_termination (&self, to: Duration)->Result<()> {
        self.hself.hsys.send_msg( ActorSystemRequest::RequestTermination, to).await
    }

    pub fn exec (&self, f: impl Fn() + Send + 'static)->Result<()> {
        self.hself.try_send_actor_msg( _Exec_(Box::new(f)).into())
    }
}

impl <S,M> Identifiable for Actor<S,M> where S: Send + 'static, M: MsgTypeConstraints {
    fn id(&self)->&str { self.id() }
}

impl <S,M> Deref for Actor<S,M> where S: Send + 'static, M: MsgTypeConstraints {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl <S,M> DerefMut for Actor<S,M> where S: Send + 'static, M: MsgTypeConstraints {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.state
    }
}

/// a surrogate for an actor that hasn't been spawned yet. This is useful to break cyclic dependencies.
/// The only purpose of PreActorHandles is to pre-allocate the channel sender/receiver and to initialize
/// ActorHandles and MsgReceivers from it. No messages can be sent through PreActorHandle
/// We cannot directly pre-alloc ActorHandles since most channel crates do not have cloneable Receivers
pub struct PreActorHandle <M> where M: MsgTypeConstraints {
    hsys: Arc<ActorSystemHandle>,
    id: Arc<String>,
    tx: MpscSender<M>,
    rx: Option<MpscReceiver<M>> // this is reset when the actor is spawned from this PreActorHandle
}

impl <M> PreActorHandle <M>  where M: MsgTypeConstraints {
    pub fn new (sys: &ActorSystem, id: impl ToString, bound: usize)->Self {
        let hsys = sys.clone_handle();
        let id = Arc::new(id.to_string());
        let (tx, rx) = create_mpsc_sender_receiver::<M>( bound);
        PreActorHandle { hsys, id, tx, rx: Some(rx) }
    }

    pub fn to_actor_handle (&self)->ActorHandle<M> {
        ActorHandle{ id: self.id.clone(), hsys: self.hsys.clone(), tx: self.tx.clone() }
    }

    pub fn get_id (&self)->Arc<String> {
        self.id.clone()
    }
}

/// we impl Drop for PreActorHandle so that we can check at the drop point if it was moved
/// into a ActorSystem::new_pre_actor(pre ..) call. If not this most likely is an application bug
/// that called spawn_actor(..) instead of spawn_pre_actor(..), which then subsequently leads
/// to disconnected errors in sends from respective PreActorHandle users (there is no receiver)
impl <M> Drop for PreActorHandle<M> where M: MsgTypeConstraints {
    fn drop (&mut self) {
        if self.rx.is_some() {
            // TODO we might want to panic here
            error!("pre actor handle {} was not spawned", self.id)
        }
    }
}

impl <M> Identifiable for PreActorHandle<M> where M: MsgTypeConstraints {
    fn id (&self) -> &str { self.id.as_str() }
}

/// this is a wrapper for the minimal data we need to send messages of type M to the respective actor
/// Note this is a partially opaque type
pub struct ActorHandle <M> where M: MsgTypeConstraints {
    pub id: Arc<String>,
    hsys: Arc<ActorSystemHandle>,
    tx: MpscSender<M> // internal - this is channel specific
}

impl <M> ActorHandle <M> where M: MsgTypeConstraints {
    pub fn get_id (&self)->Arc<String> {
        self.id.clone()
    }

    pub fn hsys(&self)->&ActorSystemHandle {
        self.hsys.as_ref()
    }

    pub fn is_running(&self) -> bool {
        !is_tx_disconnected(&self.tx)
    }

    /// this waits indefinitely until the message can be send or the receiver got closed
    pub async fn send_actor_msg (&self, msg: M)->Result<()> {
        debug!("send_actor_msg to '{}': msg: {:?}", self.id, msg);

        send( &self.tx, msg).await.map_err(|e| {
            debug!("send error {e}");
            OdinActorError::ReceiverClosed
        })
    }

    pub async fn send_msg<T> (&self, msg: T)->Result<()> where T: Into<M> {
        self.send_actor_msg( msg.into()).await
    }

    /// this version consumes self, which is handy if we send from within a closure that
    /// did capture the ActorHandle. Without it the borrow checker would complain that we
    /// borrow self within a future from our closure context
    pub async fn move_send_msg<T> (self, msg: T)->Result<()> where T: Into<M> {
        self.send_actor_msg( msg.into()).await
    }

    /// this waits for a given timeout duration until the message can be send or the receiver got closed
    pub async fn timeout_send_actor_msg (&self, msg: M, to: Duration)->Result<()> {
        debug!("with timeout {:?}", to);
        timeout( to, self.send_actor_msg(msg)).await
    }

    pub async fn timeout_send_msg<T> (&self, msg: T, to: Duration)->Result<()> where T: Into<M> {
        self.timeout_send_actor_msg( msg.into(), to).await
    }

    /// for use in closures that capture the actor handle - see [`move_send_msg`]
    pub async fn timeout_move_send_msg<T> (self, msg: T, to: Duration)->Result<()> where T: Into<M> {
        self.timeout_send_msg( msg, to).await
    }

    /// this returns immediately but the caller has to check if the message got sent
    pub fn try_send_actor_msg (&self, msg: M)->Result<()> {
        debug!( "try_send_actor_msg to '{}': msg: {:?}", self.id, msg);
        match_try_send!{ self.tx, msg,
            ok => {
                Ok(())
            }
            full => {
                warn!("receiver mailbox full");
                Err(OdinActorError::ReceiverFull)
            }
            closed => {
                warn!("receiver closed");
                Err(OdinActorError::ReceiverClosed) // ?? what about SendError::Closed 
            }
        }
    }

    pub fn try_send_msg<T> (&self, msg:T)->Result<()> where T: Into<M> {
        self.try_send_actor_msg(msg.into())
    }

    /// Note that Ok(()) just means the retry message got scheduled, not that it succeeded
    pub fn retry_send_msg<T> (&self, max_attempts: usize, delay: Duration, msg: T)->Result<()> where T: Into<M>+Clone+Send+'static {
        if let Ok(mut scheduler) = self.hsys().get_scheduler() {
            scheduler.schedule_repeated( delay, delay, {
                let mut remaining_attempts=max_attempts;
                let actor_handle=self.clone();
                move |ctx| {
                    if let Err(OdinActorError::ReceiverFull) = actor_handle.try_send_msg( msg.clone()) {
                        if remaining_attempts > 0 {
                            remaining_attempts -= 1;
                        } else { ctx.cancel_repeat() }
                    } else { ctx.cancel_repeat() }
                }
            });
            Ok(())
        } else {
            Err(op_failed("failed to schedule retry message"))
        }
    }

    #[inline(always)]
    pub fn get_scheduler (&self)->LockResult<MutexGuard<'_,JobScheduler>> {
        self.hsys.get_scheduler()
    }

    pub fn start_oneshot_timer (&self, id: i64, delay: Duration) -> Result<AbortHandle> {
        oneshot_timer_for( self.clone(), id, delay)
    }

    pub fn start_repeat_timer (&self, id: i64, timer_interval: Duration, instantly: bool) -> Result<AbortHandle> {
        repeat_timer_for( self.clone(), id, timer_interval, instantly)
    }

    pub fn exec (&self, f: impl Fn() + Send + 'static)->Result<()> {
        self.try_send_actor_msg( _Exec_(Box::new(f)).into())
    }

    pub fn new_actor<S,U> (&self, id: impl ToString, state: S, bound: usize)->(Actor<S,U>, ActorHandle<U>, MpscReceiver<U>)
        where S: Send + 'static, U: MsgTypeConstraints
    {
        actor_tuple( self.hsys.clone(), id, state, bound)
    }
}

// note this consumed the ActorHandle since we have to move it into a Future
fn oneshot_timer_for<M> (ah: ActorHandle<M>, id: i64, delay: Duration)->Result<AbortHandle> where M: MsgTypeConstraints {
    let timer_name = format!("{}-timer-{}", ah.id(), id);
    let th = spawn( &timer_name, async move {
        sleep(delay).await;
        ah.try_send_actor_msg( _Timer_{id}.into() );
    })?;
    Ok(th.abort_handle())
}

fn repeat_timer_for<M> (ah: ActorHandle<M>, id: i64, timer_interval: Duration, instantly: bool)->Result<AbortHandle> where M: MsgTypeConstraints {
    let timer_name = format!("{}-timer-{}", ah.id(), id);
    let mut interval = interval(timer_interval);
    let mut send_tick = instantly; 

    let th = spawn( &timer_name, async move {
        while ah.is_running() {
            if send_tick {
                ah.try_send_actor_msg( _Timer_{id}.into() );
            } else {
                send_tick = true;
            }

            interval.tick().await;
        }
    })?;
    Ok(th.abort_handle())
}

impl <M> Identifiable for ActorHandle<M> where M: MsgTypeConstraints {
    fn id (&self) -> &str { self.id.as_str() }
}

impl <M> Debug for ActorHandle<M> where M: MsgTypeConstraints {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ActorHandle(\"{}\")", self.id)
    }
}

impl<M> PartialEq for ActorHandle<M> where M: MsgTypeConstraints {
    fn eq (&self, other: &Self) -> bool {
        self.id == other.id // this immplies the id Arcs are pointing to the same object
    }
}

impl <M> Clone for ActorHandle <M> where M: MsgTypeConstraints {
    fn clone(&self)->Self {
        ActorHandle::<M> { id: self.id.clone(), hsys: self.hsys.clone(), tx: self.tx.clone() }
    }
}

impl<M> From<&PreActorHandle<M>> for ActorHandle<M> where M: MsgTypeConstraints {
    fn from (pre: &PreActorHandle<M>)->Self {
        ActorHandle{ id: pre.id.clone(), hsys: pre.hsys.clone(), tx: pre.tx.clone() }
    }
}

/// blanket impl of non-object-safe trait that can send anything that can be turned into a MsgType
/// (use [`DynMsgReceiver`] if this needs to be sent/stored as trait object)
impl <T,M> MsgReceiver <T> for ActorHandle <M>
    where  T: Send + Debug + 'static,  M: From<T> + MsgTypeConstraints
{
    fn send_msg (&self, msg: T) -> impl Future<Output = Result<()>> + Send {
        self.send_actor_msg( msg.into())
    }

    fn timeout_send_msg (&self, msg: T, to: Duration) -> impl Future<Output = Result<()>> + Send {
        self.timeout_send_actor_msg( msg.into(), to)
    }
}

/// blanket impl of object safe trait that can send anything that can be turned into a MsgType 
/// Note - this has to pin-box futures upon every send and hence is less efficient than [`MsgReceiver`]
/// hence this should only be used where we need sendable MsgReceivers
impl <T,M> DynMsgReceiverTrait <T> for ActorHandle <M>
    where T: Send + Debug + 'static,  M: From<T> + MsgTypeConstraints
{
    fn send_msg (&self, msg: T) -> MsgSendFuture {
        Box::pin( self.send_actor_msg( msg.into()))
    }

    fn timeout_send_msg (&self, msg: T, to: Duration) -> MsgSendFuture {
        Box::pin( self.timeout_send_actor_msg( msg.into(), to))
    }
}

impl <T,M> From<ActorHandle<M>> for DynMsgReceiver<T> 
    where T: Send + Debug + 'static,  M: From<T> + MsgTypeConstraints
{
    fn from (a:ActorHandle<M>)->DynMsgReceiver<T> {
        Box::new(a.clone())
    }
}

impl <T,M> TryMsgReceiver <T> for ActorHandle <M>
    where T: Send + Debug + 'static,  M: From<T> + MsgTypeConstraints
{
    fn try_send_msg (&self, msg: T) -> Result<()> {
        self.try_send_actor_msg( msg.into())
    }
}

impl <M> SysMsgReceiver for ActorHandle<M> where M: MsgTypeConstraints 
{
    fn send_start (&self,msg: _Start_, to: Duration)->MsgSendFuture {
        Box::pin(self.timeout_send_actor_msg(msg.into(),to)) 
    }
    fn send_pause (&self, msg: _Pause_, to: Duration)->MsgSendFuture {
        Box::pin(self.timeout_send_actor_msg(msg.into(),to)) 
    }
    fn send_resume (&self, msg: _Resume_, to: Duration)->MsgSendFuture {
        Box::pin(self.timeout_send_actor_msg(msg.into(),to)) 
    }
    fn send_terminate (&self, msg: _Terminate_, to: Duration)->MsgSendFuture {
        Box::pin(self.timeout_send_actor_msg(msg.into(),to)) 
    }
    fn send_ping (&self, msg: _Ping_)->Result<()> {
        self.try_send_actor_msg(msg.into()) 
    }
    fn send_timer (&self, msg: _Timer_)->Result<()> {
        self.try_send_actor_msg(msg.into()) 
    }
}


/* #endregion ActorHandle */

/* #region ActorSystem *****************************************************************************************/

/// abstraction layer for ActorSystem user interfaces. The methods are called by the actor system to transmit/update
/// display relevant information. This trait has to be object-safe.
/// 
/// Note the ActorSystemUITrait impl might be async and therefore we should (a) minimize the amount of data passed
/// in arguments, and (b) all the arguments have to be Send
pub trait ActorSystemUITrait where Self: Send + 'static {
    fn actors_started (&mut self);  // just a notification about an actor system state change
    fn add_actor (&mut self, id: Arc<String>, type_name: &'static str); // to let the UI populate an actor display list (names/types don't change)
    fn remove_actor (&mut self, idx: usize);
    fn no_start_actor (&mut self, idx: usize);
    fn heartbeats_started (&mut self); // just a notification about an actor system state change 
    fn heartbeat_cycle_started (&mut self, cycle: u32); // when we start a new heartbeat cycle
    fn actor_heartbeat (&mut self, idx: usize, cycle: u32, last_ns: u64); // latest heartbeat response of actor
    fn unresponsive_actor (&mut self, idx: usize); // we report that separately if we detect there was no response from an actor within cycle
    fn no_terminate_actor (&mut self, idx: usize);
    fn actors_terminated (&mut self); // just a notification about an actor system state change 
    //... more to follow
}

/// the type for ActorSystemUITrait trait objects
pub type DynActorSystemUI = Box<dyn ActorSystemUITrait>;

/// this is our **internal** per-actor data stored in the actor system. It is not supposed to leak (e.g. to a UI)
/// since it contains fields to control the actor (send system messages or abort task)
struct ActorEntry {
    id: Arc<String>,
    type_name: &'static str,
    abortable: AbortHandle,
    receiver: Box<dyn SysMsgReceiver>,
    ping_response: Arc<AtomicU64>, // see `Ping` for details (packed cycle/response-ns value)
}

#[derive(Clone)]
pub struct ActorSystemHandle {
    sender: MpscSender<ActorSystemRequest>,
    job_scheduler: Arc<Mutex<JobScheduler>>
}
impl ActorSystemHandle {
    pub async fn send_msg (&self, msg: ActorSystemRequest, to: Duration)->Result<()> {
        timeout( to, send(&self.sender, msg)).await
    }

    pub fn try_send_msg (&self, msg: ActorSystemRequest)->Result<()> {
        match_try_send!{ self.sender, msg,
            ok => { 
                Ok(()) 
            }
            full => {
                warn!("actor system request queue full");
                Err(OdinActorError::ReceiverFull)
            }
            closed => {
                warn!("actor system request queue closed");
                Err(OdinActorError::ReceiverClosed) // ?? what about SendError::Closed 
            }
        }
    }

    pub async fn spawn_actor<M,R> (&self, act: (R, ActorHandle<M>, MpscReceiver<M>))->Result<ActorHandle<M>> 
    where
        M: MsgTypeConstraints,
        R: ActorReceiver<M> + Send + Sync + 'static
    {
        let (mut receiver, actor_handle, rx) = act;
        let id = actor_handle.id.clone();
        let type_name = std::any::type_name::<R>();
        let sys_msg_receiver = Box::new(actor_handle.clone());
        let func = move || { run_actor(rx, receiver) };
        let sfc = create_sfc( func);

        self.send_msg( ActorSystemRequest::RequestActorOf { id, type_name, sys_msg_receiver, sfc }, secs(1)).await?;
        Ok(actor_handle)
    }

    pub fn get_scheduler (&self)->LockResult<MutexGuard<'_,JobScheduler>> {
        self.job_scheduler.lock()
    }

    pub async fn request_termination (&self, to: Duration)->Result<()> {
        self.send_msg( ActorSystemRequest::RequestTermination, to).await
    }
}


/// the ActorSystem representation for the function in which it is created
pub struct ActorSystem {
    id: String,
    ping_cycle: u32,
    request_sender: MpscSender<ActorSystemRequest>,
    request_receiver: MpscReceiver<ActorSystemRequest>,
    job_scheduler: Arc<Mutex<JobScheduler>>, 
    join_set: task::JoinSet<()>, 
    actor_entries: Vec<ActorEntry>,
    heartbeat_job: Option<JobHandle>,
    hsys: Arc<ActorSystemHandle>,
    ui: Option<DynActorSystemUI>
}

impl ActorSystem {

    pub fn new (id: impl ToString)->Self {
        let (tx,rx) = create_mpsc_sender_receiver(8);
        let mut job_scheduler = Arc::new( Mutex::new( JobScheduler::with_max_pending( 1024)));
        let hsys = Arc::new( ActorSystemHandle{sender: tx.clone(), job_scheduler: job_scheduler.clone()});

        debug!("actor system '{}' created", id.to_string());

        ActorSystem { 
            id: id.to_string(), 
            ping_cycle: 0,
            request_sender: tx,
            request_receiver: rx,
            job_scheduler,
            join_set: JoinSet::new(),
            actor_entries: Vec::new(),
            heartbeat_job: None,
            hsys,
            ui: None
        }
    }

    pub fn with_env_tracing (id: impl ToString)->Self {
        tracing_subscriber::fmt::init();
        Self::new(id)
    }

    pub fn set_ui (&mut self, ui: DynActorSystemUI) {
        self.ui = Some(ui);
    }

    pub fn handle (&self)->&ActorSystemHandle {
        self.hsys.as_ref()
    }

    pub fn clone_handle (&self)->Arc<ActorSystemHandle> {
        self.hsys.clone()
    }

    // these two functions need to be called at the user code level. The separation is required to guarantee that
    // there is a Receiver<M> impl for the respective Actor<S,M> - the new_(..) returns the concrete Actor<S,M>
    // and the spawn_(..) expects a Receiver<M> and hence fails if there is none in scope. The ugliness comes in form
    // of all the ActorSystem internal data we create in new_(..) but need in spawn_(..) and unfortunately we can't even use
    // the Actor hself field since spawn_(..) doesn't even see that it's an Actor (it consumes the Receiver).
    // We can't bypass Receiver by providing receive() through a fn()->impl Future<..> since impl-in-return-pos is not 
    // supported for fn pointers.
    // We also can't use a default blanket Receive impl for Actor and min_specialization - apart from that it isn't stable yet
    // it does not support async traits

    pub fn new_actor<S,M> (&self, id: impl ToString, state: S, bound: usize)->(Actor<S,M>, ActorHandle<M>, MpscReceiver<M>)
        where S: Send + 'static, M: MsgTypeConstraints
    {
        debug!("creating actor '{}'", id.to_string());
        actor_tuple( self.hsys.clone(), id, state, bound)
    }

    pub fn new_pre_actor<S,M> (&self, mut h_pre: PreActorHandle<M>, state: S)->(Actor<S,M>, ActorHandle<M>, MpscReceiver<M>)
        where S: Send + 'static, M: MsgTypeConstraints
    {
        debug!("creating pre actor'{}'", h_pre.id());
        pre_actor_tuple( self.hsys.clone(), state, h_pre)
    }

    /// although this implementation is infallible others (e.g. through an [`ActorHandle`] or using different
    /// channel types) are not. To keep it consistent we return a `Result<ActorHandle>``
    pub fn spawn_actor<R,M> (&mut self, act: (R, ActorHandle<M>, MpscReceiver<M>))->Result<ActorHandle<M>>
        where
            M: MsgTypeConstraints,
            R: ActorReceiver<M> + Send + 'static
    {
        let (mut receiver, actor_handle, rx) = act;

        //let abort_handle = self.join_set.spawn( run_actor(rx, receiver));
        let abort_handle = self.join_set.build_task()
            .name( actor_handle.id())
            .spawn( run_actor(rx, receiver))?;

        let actor_entry = ActorEntry {
            id: actor_handle.id.clone(),
            type_name: type_name::<R>(),
            abortable: abort_handle,
            receiver: Box::new(actor_handle.clone()), // stores it as a SysMsgReceiver trait object
            ping_response: Arc::new(AtomicU64::new(0)),

        };

        if let Some(ui) = &mut self.ui { ui.add_actor( actor_entry.id.clone(), actor_entry.type_name) }
        self.actor_entries.push( actor_entry);

        Ok(actor_handle)
    }

    // this is used from spawned actors sending us RequestActorOf messages
    fn spawn_actor_request (&mut self, actor_id: Arc<String>, type_name: &'static str, sys_msg_receiver: Box<dyn SysMsgReceiver>, sfc: SendableFutureCreator) {
        let abort_handle = self.join_set.spawn( sfc());
        let actor_entry = ActorEntry {
            id: actor_id,
            type_name,
            abortable: abort_handle,
            receiver: sys_msg_receiver, // stores it as a SysMsgReceiver trait object
            ping_response: Arc::new(AtomicU64::new(0)),
        };

        if let Some(ui) = &mut self.ui { ui.add_actor( actor_entry.id.clone(), actor_entry.type_name) }
        self.actor_entries.push( actor_entry);
    }

    pub fn get_scheduler (&self)->LockResult<MutexGuard<'_,JobScheduler>> {
        self.job_scheduler.lock()
    }

    // this should NOT be accessible from actors, hence we require a &mut self
    pub async fn wait_all (&mut self, to: Duration) -> Result<()> {
        let mut join_set = &mut self.join_set;

        let len = join_set.len();
        let mut closed = 0;
        while let Some(_res) = join_set.join_next().await {
            closed += 1;
        }
        
        iter_op_result("start_all", len, len-closed)   
    }

    pub async fn abort_all (&mut self) {
        let mut join_set = &mut self.join_set;
        join_set.abort_all();
    }

    pub async fn ping_all (&mut self)->Result<()> {
        self.ping_cycle += 1;

        if let Some(ui) = &mut self.ui { ui.heartbeat_cycle_started(self.ping_cycle) }

        for actor_entry in &self.actor_entries {
            let response = actor_entry.ping_response.clone();
            actor_entry.receiver.send_ping( _Ping_::new( self.ping_cycle, response));

            // give the receiver a chance to get scheduled but don't block (we don't know if it is still alive)
            yield_now().await;
            //sleep(millis(1)).await;
        }
        Ok(())
    }

    // this is called at the beginning of the next cycle but before incrementing the ping_cycle
    fn process_ping_responses (&mut self) {
        let cur_cycle: u32 = self.ping_cycle;
        //println!("--- processing ping cycle: {cur_cycle}");

        let mut idx = 0;
        for mut actor_entry in &mut self.actor_entries {
            let (cycle,last_ns) = unpack_ping_response( actor_entry.ping_response.load(Ordering::Relaxed));
            if (cycle == cur_cycle) {
                if let Some(ui) = &mut self.ui { ui.actor_heartbeat(idx, cycle, last_ns) }
            } else {
                warn!("actor {} failed to respond in ping cycle {}", actor_entry.id, cur_cycle);

                // we leave it up to the UI to terminate
                if let Some(ui) = &mut self.ui { ui.unresponsive_actor(idx) }
            }
            idx += 1;
        }
    }

    pub async fn start_all(&mut self)->Result<()> {
        self.timeout_start_all(millis(100)).await
    }

    pub async fn timeout_start_all (&mut self, to: Duration)->Result<()> {
        let actor_entries = &self.actor_entries;
        let mut failed = 0;

        self.start_scheduler();

        for (idx,actor_entry) in actor_entries.iter().enumerate() {
            if actor_entry.receiver.send_start(_Start_{}, to).await.is_err() { 
                if let Some(ui) = &mut self.ui { ui.no_start_actor(idx) }
                failed += 1 
            }
        }
        // TODO - do we need to wait until everybody has processed _Start_ ?
        iter_op_result("start_all", actor_entries.len(), failed)
    }

    pub async fn terminate_all (&mut self, to: Duration)->Result<()>  {
        let mut len = self.actor_entries.len();
        let mut failed = 0;

        self.stop_scheduler();

        //for actor_entry in self.actors.iter().rev() { // send terminations in reverse ?
        for (idx,actor_entry) in self.actor_entries.iter().enumerate() {
            if actor_entry.receiver.send_terminate(_Terminate_{}, to).await.is_err() { 
                if let Some(ui) = &mut self.ui { ui.no_terminate_actor(idx) }
                failed += 1 
            };
        }

        // no need to wait for responses since we use the join_set to sync
        iter_op_result("terminate_all", len, failed)
    }

    pub async fn terminate_and_wait (&mut self, to: Duration)->Result<()> {
        self.terminate_all( to).await;

        let res = self.wait_all(to).await;
        if (res.is_err()) {
            self.abort_all().await
        }
    
        res
    }

    pub fn stop_scheduler (&self) {
        if let Ok(mut scheduler) = self.get_scheduler() { // TODO - should this be done here
            scheduler.abort();
        }
    }

    pub fn start_scheduler (&self) {
        if let Ok(mut scheduler) = self.get_scheduler() { // TODO - should this be done here
            scheduler.run();
        }
    }

    // this is where we process ActorSystemRequests until the system has terminated
    pub async fn process_requests (&mut self)->Result<()> {
        debug!("actor system '{}' running", self.id);

        loop {
            match recv(&self.request_receiver).await {
                Ok(msg) => {
                    debug!("actor system '{}' processing request: {:?}", self.id, msg);
                    match msg {
                        ActorSystemRequest::RequestTermination => {
                            self.terminate_and_wait(secs(5)).await?;
                            break;
                        }
                        ActorSystemRequest::RequestHeartbeat => {
                            if (self.ping_cycle > 0) {
                                self.process_ping_responses();
                            }
                            self.ping_all().await;
                        }
                        ActorSystemRequest::RequestActorOf { id, type_name, sys_msg_receiver, sfc } => {
                            self.spawn_actor_request( id, type_name, sys_msg_receiver, sfc)
                        }
                    }
                }
                Err(_) => {
                    return Err(OdinActorError::ReceiverClosed) // ??
                }
            }
        }

        debug!("actor system '{}' terminated", self.id);
        Ok(())
    }

    pub fn start_heartbeats (&mut self, interval: Duration)->Result<()> {
        if self.heartbeat_job.is_none() {
            let hsys = self.hsys.clone();
            if let Ok(mut scheduler) = self.job_scheduler.lock() {
                let job_handle = scheduler.schedule_repeated( Duration::ZERO, interval, move |_ctx| {
                    hsys.try_send_msg(ActorSystemRequest::RequestHeartbeat{});
                })?;
                debug!("heartbeat task started");
                self.heartbeat_job.replace(job_handle);
                Ok(())
            } else {
                Err(op_failed("scheduling heartbeat job failed"))
            }
        } else { // already scheduled
            warn!("heartbeat task already running");
            Ok(())
        }
    }

    pub async fn process_requests_for (&mut self, dur: Duration)->Result<()> {
        let hsys = self.hsys.clone();
        if let Ok(mut scheduler) = self.job_scheduler.lock() {
            scheduler.schedule_once( dur, move |_| { 
                hsys.try_send_msg(ActorSystemRequest::RequestTermination{});
            })?;
        }
        self.process_requests().await
    }

    /// set a ctrlc signal handler that sends a RequestTermination instead of just bluntly exiting
    /// the process. To be used if we have actors that need to shut down gracefully (e.g. store state)
    pub fn request_termination_on_ctrlc (&self) {
        let hsys = self.clone_handle();
        odin_common::process::set_ctrlc_handler( move || {
            hsys.try_send_msg( ActorSystemRequest::RequestTermination);
        })
    }
}

type ActorTuple<S,M> = (Actor<S,M>, ActorHandle<M>, MpscReceiver<M>);

fn actor_tuple<S,M> (hsys: Arc<ActorSystemHandle>, id: impl ToString, state: S, bound: usize)->ActorTuple<S,M>
    where S: Send + 'static, M: MsgTypeConstraints
{
    let actor_id = Arc::new(id.to_string());
    let (tx, rx) = create_mpsc_sender_receiver::<M>( bound);
    let actor_handle = ActorHandle { id: actor_id, hsys, tx };
    let hself = actor_handle.clone();
    let actor = Actor{ state, hself };

    (actor, actor_handle, rx)
}

fn pre_actor_tuple<S,M> (hsys: Arc<ActorSystemHandle>, state: S, mut pre_h: PreActorHandle<M>)->ActorTuple<S,M>
    where S: Send + 'static, M: MsgTypeConstraints
{
    let actor_id = pre_h.id.clone();

    if pre_h.rx.is_none() { 
        error!("pre actor already spawned: {}", actor_id);
    }    

    let rx = pre_h.rx.take().unwrap(); // there should always be just one receiver or we compromise actor integrity
    let tx = pre_h.tx.clone();

    let actor_handle = ActorHandle{ id: actor_id, hsys, tx };
    let hself = actor_handle.clone();
    let actor = Actor{ state, hself };

    (actor, actor_handle, rx)
}

async fn run_actor<M,R> (mut rx: MpscReceiver<M>, mut receiver: R)
    where
        M: MsgTypeConstraints,
        R: ActorReceiver<M> + Send + 'static
{
    debug!("actor '{}' running", receiver.id());

    loop {
        match recv(&rx).await {
            Ok(msg) => {
                debug!("actor '{}' processing msg: {:?}", receiver.id(), msg);
                match receiver.receive(msg).await {
                    ReceiveAction::Continue => {
                        debug!("msg processed");
                    } 
                    ReceiveAction::Stop => {
                        debug!("actor '{}' closed", receiver.id());
                        close_rx( &rx);
                        break;
                    }
                    ReceiveAction::RequestTermination => {
                        receiver.hsys().send_msg(ActorSystemRequest::RequestTermination, secs(1)).await;
                    }
                }
            }
            Err(_) => break // TODO shall we treat ReceiveError::Closed and ::SendClosed the same? what if there are no senders yet?
        }
    }

    debug!("actor '{}' terminated", receiver.id());

    // TODO - remove actor entry from ActorSystemData
}

/* #endregion ActorSystem */

/* #region Queries *********************************************************************************************/

/// struct that abstracts synchronous 1:1 roundtrip messages. The sender is blocked until it receives a (single)
/// response through a dedicated response channel that is encapsulated in the Query instance.
/// Note this should only be used with timeouts or in cases where the receiver is guaranteed to respond in
/// bounded time, to avoid blocking the query originator receive() loop.
/// If that cannot be guaranteed the query should be moved into a background task.
pub struct Query<Q,A> where Q: Send + Debug, A: Send + Debug {
    pub question: Q,
    tx: MpscSender<A>
}

impl <Q,A> Query<Q,A> where Q: Send + Debug, A: Send + Debug + 'static {

    /// respond to the query with an answer
    /// note that we do not consume self anymore so that we still have access to the query object
    /// in the caller (e.g. if queries are stored in collections). While this means we could send
    /// several responses for the same query to a receiver that only processes one, we would get
    /// an error result. This is similar to the case where the receiver does not await our response.
    pub async fn respond (&self, answer: A) -> Result<()> {
        send( &self.tx, answer).await.map_err(|_| OdinActorError::ReceiverClosed)
    }
}

impl<Q,A> Debug for Query<Q,A>  where Q: Send + Debug, A: Send + Debug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Request<{},{}>{:?}", type_name::<Q>(), type_name::<A>(), self.question)
    }
}

pub async fn query<Q,A,R> (responder: R, topic: Q)->Result<A> 
    where Q: Send + Debug, A: Send + Debug, R: MsgReceiver<Query<Q,A>>
{
    let qb = QueryBuilder::<A>::new();
    qb.query( responder, topic).await
}

pub async fn query_ref<Q,A,R> (responder: &R, topic: Q)->Result<A> 
    where Q: Send + Debug, A: Send + Debug, R: MsgReceiver<Query<Q,A>> + Sync
{
    let qb = QueryBuilder::<A>::new();
    qb.query_ref( responder, topic).await
}

/// oneshot timeout query
pub async fn timeout_query<Q,A,R> (responder: R, topic: Q, to: Duration)->Result<A> 
    where Q: Send + Debug, A: Send + Debug, R: MsgReceiver<Query<Q,A>>
{
    let qb = QueryBuilder::<A>::new();
    qb.timeout_query( responder, topic, to).await
}

pub async fn timeout_query_ref<Q,A,R> (responder: &R, topic: Q, to: Duration)->Result<A> 
    where Q: Send + Debug, A: Send + Debug, R: MsgReceiver<Query<Q,A>> + Sync
{
    let qb = QueryBuilder::<A>::new();
    qb.timeout_query_ref( responder, topic, to).await
}


/// builder for Query instances that avoids the extra cost of a per-request channel allocation for repeated queries 
/// of the same answer type and is therefore slightly faster compared to a per-query Oneshot channel
pub struct QueryBuilder<A>  where A: Send + Debug {
    tx: MpscSender<A>,
    rx: MpscReceiver<A>,
}

impl <A> QueryBuilder<A> where A: Send + Debug {
    pub fn new ()->Self {
        let (tx,rx) = create_mpsc_sender_receiver::<A>(0);
        QueryBuilder { tx, rx }
    }

    pub async fn query <Q,R> (&self, responder: R, topic: Q)->Result<A> 
        where Q: Send + Debug, R: MsgReceiver<Query<Q,A>>
    {
        let msg = Query { question: topic, tx: self.tx.clone() };
        responder.send_msg(msg).await;
        recv(&self.rx).await.map_err(|_| OdinActorError::SendersDropped)
    }

    /// if we use this version `M` has to be `Send` + `Sync` but we save the cost of cloning the responder on each query
    pub async fn query_ref <Q,R> (&self, responder: &R, topic: Q)->Result<A> 
        where Q: Send + Debug, R: MsgReceiver<Query<Q,A>> + Sync
    {
        let msg = Query { question: topic, tx: self.tx.clone() };
        responder.send_msg(msg).await;
        recv(&self.rx).await.map_err(|_| OdinActorError::SendersDropped)
    }

    pub async fn timeout_query <Q,R> (&self, responder: R, topic: Q, to: Duration)->Result<A> 
        where Q: Send + Debug, R: MsgReceiver<Query<Q,A>>
    {
        timeout( to, self.query( responder, topic)).await
    }

    /// if we use this version `M` has to be `Send` + `Sync` but we save the cost of cloning the responder on each query
    pub async fn timeout_query_ref <Q,R> (&self, responder: &R, topic: Q, to: Duration)->Result<A> 
        where Q: Send + Debug, R: MsgReceiver<Query<Q,A>> + Sync
    {
        timeout( to, self.query_ref( responder, topic)).await
    }
}

/* #endregion QueryBuilder & Query */

/* #region RequestProcessor ************************************************************************************/

/// trait to process queued, possibly overlapping queries in a background task.
/// The use case is to support long running queries that should be resolved sequentially, e.g. to avoid
/// external server overload or high computational loads.
/// This might cause several queries for the same subject to arrive (also from different requesters) while the
/// answer is still computed, which in turn requires to keep track of all requesters we have to respond to 
/// once the answer is available.
/// This means we have to be able to check if two queries are about the same subject (hence `is_same_request()`) and
/// we have to be able to duplicate the answer (hence A: Clone).
/// The resolver task has to loop over a select() since it has to await both new queue requests and completed
/// answers. The logic of this select loop is implemented in the generic (internal) process_requests() function.
///
/// example sequence diagram:
///                                                 responder
///      task-1    task-2     task-3                  task
///        :         :          :                      :
///        []--Q1--- : -------- : ------------(queue)->[]... 
///        []        []---Q1--- : ------------(queue)->[] Q1
///        []        []         []---Q2-------(queue)->[]
///        []<------ []<------- [] --------------A1----[]...
///        :         :          []                     [] Q2
///        :         []---Q3--- [] -----------(queue)->[]
///        :         []         []<--------------A2----[]...
///        :         []         :                      [] Q3
///        :         []<------- : ---------------A3----[]...
///        :         :          :                      :
///
pub trait RequestProcessor<R,T> 
    where Self: Sized + Send + 'static,
          R: Send + Sync + Debug + 'static, // request type
          T: Clone + Send + Debug + 'static // result type
{
    // we need an initial future before we get the first query, hence the option arg. Note its output is never used
    fn get_response_future (&self, request: Option<R>) -> impl Future<Output=Option<(R,T)>> + Send;
    fn process_response (&self, request: &R, answer: T) -> impl Future<Output=Result<()>> + Send;
    fn is_same_request (&self, request1: &R, request2: &R)->bool; // we could also turn this into a PartialEq constraint on R
    
    /// provided method to loop over pending requests
    fn spawn (self, task_name: &str, bounds: usize)->Result<(AbortHandle,MpscSender<R>)> {
        let (tx,rx) = create_mpsc_sender_receiver::<R>(bounds);
        let jh = spawn( task_name, async move {
            process_requests(self,rx).await
        })?;
        Ok( (jh.abort_handle(), tx) )
    }
}

async fn process_requests<P,R,T> (proc: P, rx: MpscReceiver<R>) -> Result<()> 
    where R: Send + Sync + Debug + 'static, // request type
          T: Clone + Send + Debug + 'static, // result type
          P: RequestProcessor<R,T> + Sized + 'static
{
    let mut response_pending = false;
    let mut pending_requests: VecDeque<R> = VecDeque::new();
    let mut response_fut = proc.get_response_future(None);
    tokio::pin!(response_fut);

    loop {
        tokio::select! {
            // note that we use a reference to resolve_fut so that we can keep it if pending.
            // note also that we have to use a guard to ensure we don't double poll a ready initial future
            res = &mut response_fut, if response_pending => {
                if let Some((request,response)) = res { // future did resolve to a Result<A>                    
                    let mut i = 0;
                    while i < pending_requests.len() {
                        let req = &pending_requests[i];
                        if proc.is_same_request( req, &request) {
                            if let Err(e) = proc.process_response( req, response.clone()).await {
                                error!("processing response for {req:?} failed: {}", e.to_string())
                            }
                            pending_requests.remove(i);
                        } else {
                            i += 1;
                        }
                    }

                    if let Err(e) = proc.process_response( &request, response).await {
                        error!("processing response for {request:?} failed: {}", e.to_string())
                    }

                    response_pending = !pending_requests.is_empty();
                    if response_pending {
                        response_fut.set( proc.get_response_future( pending_requests.pop_front()));
                    }

                    yield_now().await; // let active tasks react before we process the next query
                } // ignore if response_fut output was None
            }

            res = recv( &rx) => {
            //res = rx.recv() => {
                match res {
                    Ok(new_request) => { // got new request
                        if !response_pending { // we need a new resolve future
                            response_pending = true;
                            response_fut.set( proc.get_response_future( Some( new_request)));
                        } else {
                            pending_requests.push_back( new_request);
                        }
                    }
                    Err(e) => {
                        return Err(OdinActorError::ReceiverClosed)
                    }
                }
            }
        }
    }
    Ok(())
}

/* #endregion RequestResolver */

/* #region DRY macros ****************************************************************************/

// don't use a tt here since it would not show errors within it - only the whole body would get rejected
// slightly reduces readability since it has to be used as `run_async_main({...});` but for that we get
// precise compiler errors in the provided block
#[macro_export]
macro_rules! run_async_main {
    ( $body:expr ) => {
        use tokio;
        use anyhow;

        #[tokio::main]
        async fn main ()->anyhow::Result<()> {
            Ok( $body? )
        }
    }
}

#[macro_export]
macro_rules! run_actor_system {
    ($asys:ident => $set_up:expr) => {
        use tokio;
        use anyhow;

        #[tokio::main]
        async fn main ()->anyhow::Result<()> {
            odin_build::set_bin_context!();
            let mut $asys = ActorSystem::with_env_tracing("main");
            $asys.request_termination_on_ctrlc();

            let _res: anyhow::Result<()> = $set_up;
            _res?;

            $asys.timeout_start_all(secs(2)).await?;
            $asys.process_requests().await?;

            Ok(())
        }
    }
}

/* #endregion  DRY macros */