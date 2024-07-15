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

use std::{fmt::Debug, future::{ready, Future, Ready}, marker::PhantomData, ops::{Deref, DerefMut, Fn}, pin::Pin, time::Duration};
use paste::paste;
use tracing_subscriber::registry::Data;
use crate::{DynMsgReceiverTrait, DynMsgReceiver, MsgReceiver,errors::{Result, OdinActorError}};


/* #region MsgReceiverList ********************************************************************************/

pub trait MsgReceiverList<T> where T: Send + Clone + Debug, Self: Send {
    fn send_msg (&self, msg: T, ignore_err: bool)->impl Future<Output=Result<()>> + Send;
    fn timeout_send_msg (&self, msg: T, to: Duration, ignore_err: bool)->impl Future<Output=Result<()>> + Send;
    fn try_send_msg (&self, msg:T, ignore_err: bool)->Result<()>;
} 

#[macro_export]
macro_rules! msg_receiver_list {
    (@inc $n:ident, $v:tt) => {
        $n += 1
    };
    ( $( $recv:ident $(. $op:ident ())? ),* : MsgReceiver < $msg_t:ty > ) => {
        paste::paste! {
            {
                let mut len=0;
                $( msg_receiver_list!(@inc len, $recv); )*

                struct SomeMsgReceiverList < $( [<T $recv>]: MsgReceiver<$msg_t> ),* > { $( $recv: [<T $recv>], )* len:usize }

                impl< $( [<T $recv>] : MsgReceiver<$msg_t>),* > MsgReceiverList <$msg_t> for SomeMsgReceiverList < $( [<T $recv>] ),* > {
                    async fn send_msg (&self, msg: $msg_t, ignore_err: bool)->Result<()> {
                        let mut i=1;
                        if ignore_err { 
                            $(
                                if i < self.len {
                                    let _ = self.$recv.send_msg( msg.clone()).await; i += 1
                                } else {
                                    let _ = self.$recv.send_msg( msg).await; return Ok(())
                                }
                            )* 
                        } else {
                            $(
                                if i < self.len {
                                    self.$recv.send_msg( msg.clone()).await?; i += 1
                                } else {
                                    return self.$recv.send_msg( msg).await
                                }
                            )*
                        }
                        Ok(()) 
                    }
                    async fn timeout_send_msg (&self, msg: $msg_t, to: std::time::Duration, ignore_err: bool)->Result<()> {
                        let mut i=1;
                        if ignore_err { 
                            $(
                                if i < self.len {
                                    let _ = self.$recv.timeout_send_msg( msg.clone(), to).await; i += 1
                                } else {
                                    let _ = self.$recv.timeout_send_msg( msg, to).await; return Ok(())
                                }
                            )* 
                        } else {
                            $(
                                if i < self.len {
                                    self.$recv.timeout_send_msg( msg.clone(), to).await?; i += 1
                                } else {
                                    return self.$recv.timeout_send_msg( msg, to).await
                                }
                            )*
                        }
                        Ok(()) 
                    }
                    fn try_send_msg (&self, msg: $msg_t, ignore_err: bool)->Result<()> {
                        let mut i=1;
                        if ignore_err { 
                            $(
                                if i < self.len {
                                    let _ = self.$recv.try_send_msg( msg.clone()); i += 1
                                } else {
                                    let _ = self.$recv.try_send_msg( msg); return Ok(())
                                }
                            )* 
                        } else {
                            $(
                                if i < self.len {
                                    self.$recv.try_send_msg( msg.clone())?; i += 1
                                } else {
                                    return self.$recv.try_send_msg( msg)
                                }
                            )*
                        }
                        Ok(()) 
                    }
                }

                SomeMsgReceiverList{ $( $recv: $recv $(. $op () )?, )* len }
            }
        }
    }
}

pub struct DynMsgReceiverList<T> where T: Send + Clone + Debug {
    entries: Vec<DynMsgReceiver<T>>
}

impl<T> DynMsgReceiverList<T> where T: Send + Clone + Debug {
    pub fn new()->Self {
        DynMsgReceiverList { entries: Vec::new() }
    }

    pub async fn send_msg (&self, msg: T, ignore_err: bool)->Result<()> {
        if !self.is_empty() {
            let n = self.entries.len()-1;
            if ignore_err {
                for i in 0..n { let _ = self.entries[i].send_msg(msg.clone()).await; }
                let _ = self.entries[n].send_msg(msg).await;
            } else {
                for i in 0..n { self.entries[i].send_msg(msg.clone()).await?; }
                self.entries[n].send_msg(msg).await?;
            }
        }
        Ok(())
    }

    pub async fn timeout_send_msg (&self, msg: T, to: Duration, ignore_err: bool)->Result<()> {
        if !self.is_empty() {
            let n = self.entries.len()-1;
            if ignore_err {
                for i in 0..n { let _ = self.entries[i].timeout_send_msg(msg.clone(), to).await; }
                let _ = self.entries[n].timeout_send_msg(msg, to).await;
            } else {
                for i in 0..n { self.entries[i].timeout_send_msg(msg.clone(), to).await?; }
                self.entries[n].timeout_send_msg(msg, to).await?;
            }
        }
        Ok(())
    }

    pub fn try_send_msg (&self, msg:T, ignore_err: bool)->Result<()> {
        if !self.is_empty() {
            let n = self.entries.len()-1;
            if ignore_err {
                for i in 0..n { let _ = self.entries[i].try_send_msg(msg.clone()); }
                let _ = self.entries[n].try_send_msg(msg);
            } else {
                for i in 0..n { self.entries[i].try_send_msg(msg.clone())?; }
                self.entries[n].try_send_msg(msg)?;
            }
        }
        Ok(())
    }
}

impl <T> Deref for DynMsgReceiverList<T> where T: Send + Clone + Debug {
    type Target = Vec<DynMsgReceiver<T>>;
    fn deref(& self) -> &Self::Target {
        &self.entries
    }
}

impl <T> DerefMut for DynMsgReceiverList<T> where T: Send + Clone + Debug {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.entries
    }
}

/* #endregion MsgReceiverList */

// Data[Ref]Actions have been generalized and moved to odin_action (which is automatically re-exported by odn_actor)