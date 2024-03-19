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
#![feature(trait_alias)]

use odin_job::{JobHandle, JobScheduler};
use tokio::{
    time::{self,Interval,interval},
    task::{self, JoinSet, LocalSet},
    runtime::Handle
};
use flume::{
    bounded, Sender, Receiver, TrySendError, TryRecvError
};
use std::{
    any::type_name, boxed::Box, cell::Cell, fmt::Debug, future::Future, marker::{PhantomData, Sync}, 
    ops::{Deref,DerefMut}, pin::Pin, result::Result as StdResult, 
    sync::{atomic::{AtomicU64, Ordering}, Arc, LockResult, Mutex, MutexGuard}, time::{Duration, Instant}
};
use crate::{
    create_sfc, errors::{iter_op_result, op_failed, poisoned_lock, OdinActorError, Result}, micros, millis, nanos, secs, unpack_ping_response,
    ActorReceiver, ActorSystemRequest, DefaultReceiveAction, DynMsgReceiver, FromSysMsg, Identifiable, MsgReceiver, MsgReceiverConstraints, 
    MsgSendFuture, MsgTypeConstraints, ObjSafeFuture, ReceiveAction, SendableFutureCreator, SysMsgReceiver, TryMsgReceiver, 
    _Exec_, _Pause_, _Ping_, _Resume_, _Start_, _Terminate_, _Timer_,
    trace,debug,info,warn,error
};
use odin_macro::fn_mut;

/* #region runtime abstractions ***************************************************************************/
/*
 * This section is (mostly) for type and function aliases that allow us to program our own structs/traits/impls
 * without having to explicitly use runtime or channel crate specifics. While this means we still have
 * runtime/channel specific Actors, ActorHandles etc. their source code is (mostly) similar. 
 * Trying to hoist our actor constructs to crate level would require generic types that make code less readable
 * and still result in more runtime overhead (boxing/unboxing trait objects etc.). Moreover, it is not even
 * desirable to hoist some constructs since they are not compatible between runtime/channel implementations.
 */

pub type MpscSender<M> = Sender<M>;
pub type MpscReceiver<M> = Receiver<M>;
pub type AbortHandle = task::AbortHandle;
pub type JoinHandle<T> = task::JoinHandle<T>;

#[inline]
pub fn create_mpsc_sender_receiver <MsgType> (bound: usize) -> (MpscSender<MsgType>,MpscReceiver<MsgType>)
    where MsgType: Send
{
    bounded::<MsgType>(bound)
}

#[inline]
pub async fn sleep (dur: Duration) {
    time::sleep(dur).await;
}

#[inline]
pub async fn timeout<F,R,E> (to: Duration, fut: F)->Result<R> where F: Future<Output=StdResult<R,E>> {
    match time::timeout( to, fut).await {
        Ok(result) => result.map_err(|_| OdinActorError::SendersDropped),
        Err(e) => Err(OdinActorError::TimeoutError(to))
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
                        return Err(OdinActorError::TimeoutError(to))
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

    #[inline(always)]
    pub fn start_repeat_timer (&self, id: i64, timer_interval: Duration) -> Result<AbortHandle> {
        repeat_timer_for( self.hself.clone(), id, timer_interval)
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
    rx: MpscReceiver<M>
}

impl <M> PreActorHandle <M>  where M: MsgTypeConstraints {
    pub fn new (sys: &ActorSystem, id: impl ToString, bound: usize)->Self {
        let hsys = sys.clone_handle();
        let id = Arc::new(id.to_string());
        let (tx, rx) = create_mpsc_sender_receiver::<M>( bound);
        PreActorHandle { hsys, id, tx, rx }
    }

    pub fn as_actor_handle (&self)->ActorHandle<M> {
        ActorHandle{ id: self.id.clone(), hsys: self.hsys.clone(), tx: self.tx.clone() }
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
    pub fn hsys(&self)->&ActorSystemHandle {
        self.hsys.as_ref()
    }

    pub fn is_running(&self) -> bool {
        !self.tx.is_disconnected()
    }

    /// this waits indefinitely until the message can be send or the receiver got closed
    pub async fn send_actor_msg (&self, msg: M)->Result<()> {
        debug!("send_actor_msg to '{}': msg: {:?}", self.id, msg);
        self.tx.send_async(msg).await.map_err(|_| OdinActorError::ReceiverClosed)
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
        match self.tx.try_send(msg) {
            Ok(()) => {
                Ok(())
            }
            Err(TrySendError::Full(_)) => {
                warn!("receiver mailbox full");
                Err(OdinActorError::ReceiverFull)
            }
            Err(TrySendError::Disconnected(_)) => {
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

    // TODO - is this right to skip if we can't send? Maybe that should be an option

    pub fn start_oneshot_timer (&self, id: i64, delay: Duration) -> Result<AbortHandle> {
        oneshot_timer_for( self.clone(), id, delay)
    }

    pub fn start_repeat_timer (&self, id: i64, timer_interval: Duration) -> Result<AbortHandle> {
        repeat_timer_for( self.clone(), id, timer_interval)
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

fn repeat_timer_for<M> (ah: ActorHandle<M>, id: i64, timer_interval: Duration)->Result<AbortHandle> where M: MsgTypeConstraints {
    let timer_name = format!("{}-timer-{}", ah.id(), id);
    let mut interval = interval(timer_interval);

    let th = spawn( &timer_name, async move {
        while ah.is_running() {
            interval.tick().await;
            if ah.is_running() {
                ah.try_send_actor_msg( _Timer_{id}.into() );
            }
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
impl <T,M> DynMsgReceiver <T> for ActorHandle <M>
    where T: Send + Debug + 'static,  M: From<T> + MsgTypeConstraints
{
    // TODO - explore if we can use special allocators to mitigate runtime costs

    fn send_msg (&self, msg: T) -> MsgSendFuture {
        Box::pin( self.send_actor_msg( msg.into()))
    }

    fn timeout_send_msg (&self, msg: T, to: Duration) -> MsgSendFuture {
        Box::pin( self.timeout_send_actor_msg( msg.into(), to))
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

/* #region ActorSystem ************************************************************************************/

#[derive(Debug)]
struct PingStats {
    min_ns: u64,
    max_ns: u64,
    avg_ns: u64,
    was_outlier: bool,
    outlier: usize,
    // we could add variance here
}
impl PingStats {
    fn new ()->Self { PingStats{ min_ns: 0, max_ns: 0, avg_ns: 0, was_outlier: false, outlier: 0 } }

    fn update (&mut self, cycle: u32, last_ns: u64) {
        if cycle > 1 {
            if last_ns > 10* self.avg_ns && !self.was_outlier { // ignore one outlier
                //println!("@@ outlier: {}", last_ns);
                self.outlier += 1;
                self.was_outlier = true;

            } else {
                if last_ns < self.min_ns { self.min_ns = last_ns }
                if last_ns > self.max_ns { self.max_ns = last_ns }
        
                // we could round here but 1 nano_sec is already more resolution than realistic
                if last_ns > self.avg_ns {
                    self.avg_ns = self.avg_ns + (last_ns - self.avg_ns)/cycle as u64;
                } else {
                    self.avg_ns = self.avg_ns - (self.avg_ns - last_ns)/cycle as u64;
                }

                self.was_outlier = false;
            }
        } else {
            self.min_ns = last_ns; self.max_ns = last_ns; self.avg_ns = last_ns;
        }
    }
}

struct ActorEntry {
    id: Arc<String>,
    type_name: &'static str,
    abortable: AbortHandle,
    receiver: Box<dyn SysMsgReceiver>,
    ping_response: Arc<AtomicU64>, // see `Ping` for details
    ping_stats: PingStats
}

#[derive(Clone)]
pub struct ActorSystemHandle {
    sender: MpscSender<ActorSystemRequest>,
    job_scheduler: Arc<Mutex<JobScheduler>>
}
impl ActorSystemHandle {
    pub async fn send_msg (&self, msg: ActorSystemRequest, to: Duration)->Result<()> {
        timeout( to, self.sender.send_async(msg)).await
    }

    pub fn try_send_msg (&self, msg: ActorSystemRequest)->Result<()> {
        match self.sender.try_send(msg) {
            Ok(()) => {
                Ok(())
            }
            Err(TrySendError::Full(_)) => {
                warn!("actor system request queue full");
                Err(OdinActorError::ReceiverFull)
            }
            Err(TrySendError::Disconnected(_)) => {
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
    hsys: Arc<ActorSystemHandle>
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
            hsys
        }
    }

    pub fn with_env_tracing (id: impl ToString)->Self {
        tracing_subscriber::fmt::init();
        Self::new(id)
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

    pub fn new_pre_actor<S,M> (&self, h_pre: PreActorHandle<M>, state: S)->(Actor<S,M>, ActorHandle<M>, MpscReceiver<M>)
        where S: Send + 'static, M: MsgTypeConstraints
    {
        debug!("creating actor '{}'", h_pre.id());
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
            ping_stats: PingStats::new()

        };
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
            ping_stats: PingStats::new()
        };

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

        for actor_entry in &self.actor_entries {
            let response = actor_entry.ping_response.clone();
            actor_entry.receiver.send_ping( _Ping_::new( self.ping_cycle, response));

            // give the receiver a chance to get scheduled but don't block (we don't know if it is still alive)
            yield_now().await;
            //sleep(millis(100)).await;
        }
        Ok(())
    }

    // this is called at the beginning of the next cycle but before incrementing the ping_cycle
    fn process_ping_responses (&mut self) {
        let cur_cycle: u32 = self.ping_cycle;
        println!("--- processing ping cycle: {cur_cycle}");

        for mut actor_entry in &mut self.actor_entries {
            let (cycle,last_ns) = unpack_ping_response( actor_entry.ping_response.load(Ordering::Relaxed));
            if (cycle == cur_cycle) {
                actor_entry.ping_stats.update( cur_cycle, last_ns);

                let stats = &actor_entry.ping_stats;
                println!("{:<10} = {:>6} ns,  avg: {:>5}, min: {:>5}, max: {:>6}, outlier: {:>2}", 
                           actor_entry.id, last_ns, stats.avg_ns, stats.min_ns, stats.max_ns, stats.outlier)
            } else {
                println!("ACTOR FAILED TO RESPOND!");
            }
        }
    }

    pub fn start_all(&self)->impl Future<Output=Result<()>> {
        self.timeout_start_all(millis(100))
    }

    pub async fn timeout_start_all (&self, to: Duration)->Result<()> {
        let actor_entries = &self.actor_entries;
        let mut failed = 0;

        self.start_scheduler();

        for actor_entry in actor_entries {
            if actor_entry.receiver.send_start(_Start_{}, to).await.is_err() { failed += 1 }
        }
        // TODO - do we need to wait until everybody has processed _Start_ ?
        iter_op_result("start_all", actor_entries.len(), failed)
    }

    pub async fn terminate_all (&self, to: Duration)->Result<()>  {
        let mut len = 0;
        let mut failed = 0;

        self.stop_scheduler();

        //for actor_entry in self.actors.iter().rev() { // send terminations in reverse ?
        for actor_entry in self.actor_entries.iter() {
            len += 1;
            if actor_entry.receiver.send_terminate(_Terminate_{}, to).await.is_err() { failed += 1 };
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
            match self.request_receiver.recv_async().await {
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

fn pre_actor_tuple<S,M> (hsys: Arc<ActorSystemHandle>, state: S, pre_h: PreActorHandle<M>)->ActorTuple<S,M>
    where S: Send + 'static, M: MsgTypeConstraints
{
    let actor_id = pre_h.id.clone();
    let rx = pre_h.rx;
    let actor_handle = ActorHandle{ id: actor_id, hsys, tx: pre_h.tx };
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
        match rx.recv_async().await {
            Ok(msg) => {
                debug!("actor '{}' processing msg: {:?}", receiver.id(), msg);
                match receiver.receive(msg).await {
                    ReceiveAction::Continue => {} // just go on
                    ReceiveAction::Stop => {
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

/* #region Queries ***************************************************************************/

/// QueryBuilder avoids the extra cost of a per-request channel allocation and is therefore slightly faster
/// compared to a per-query Oneshot channel
pub struct QueryBuilder<A>  where A: Send + Debug {
    tx: MpscSender<A>,
    rx: MpscReceiver<A>,
}

impl <A> QueryBuilder<A> where A: Send + Debug {
    pub fn new ()->Self {
        let (tx,rx) = create_mpsc_sender_receiver::<A>(1);
        QueryBuilder { tx, rx }
    }

    pub async fn query <Q,R> (&self, responder: R, topic: Q)->Result<A> 
        where Q: Send + Debug, R: MsgReceiver<Query<Q,A>>
    {
        let msg = Query { question: topic, tx: self.tx.clone() };
        responder.send_msg(msg).await;
        self.rx.recv_async().await.map_err(|_| OdinActorError::SendersDropped)
    }

    /// if we use this version `M` has to be `Send` + `Sync` but we save the cost of cloning the responder on each query
    pub async fn query_ref <Q,R> (&self, responder: &R, topic: Q)->Result<A> 
        where Q: Send + Debug, R: MsgReceiver<Query<Q,A>> + Sync
    {
        let msg = Query { question: topic, tx: self.tx.clone() };
        responder.send_msg(msg).await;
        self.rx.recv_async().await.map_err(|_| OdinActorError::SendersDropped)
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

///
pub struct Query<Q,A> where Q: Send + Debug, A: Send + Debug {
    pub question: Q,
    tx: MpscSender<A>
}

impl <Q,A> Query<Q,A> where Q: Send + Debug, A: Send + Debug {
    pub async fn respond (self, answer: A)->Result<()> {
        self.tx.send_async(answer).await.map_err(|_| OdinActorError::ReceiverClosed)
    }
}

impl<Q,A> Debug for Query<Q,A>  where Q: Send + Debug, A: Send + Debug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Request<{},{}>{:?})", type_name::<Q>(), type_name::<A>(), self.question)
    }
}

/// oneshot query
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

/* #endregion QueryBuilder & Query */

// we ditch the OneshotQuery (using a oneshot channel) since it doesn't really save us anything