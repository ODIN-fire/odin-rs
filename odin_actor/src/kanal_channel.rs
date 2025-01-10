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

// items that abstract kanal MPSC channels
// note this get conditionally included into the respective runtime module 

#[cfg(any(feature="tokio_flume"))]
compile_error!("\"tokio_kanal\" and \"tokio_flume\" are exclusive");


use kanal::{ bounded_async,AsyncSender,AsyncReceiver, SendFuture, SendError, ReceiveFuture };

pub type MpscSender<M> = AsyncSender<M>;
pub type MpscReceiver<M> =AsyncReceiver<M>;

#[inline] 
pub fn create_mpsc_sender_receiver <MsgType> (bound: usize) -> (MpscSender<MsgType>,MpscReceiver<MsgType>)
    where MsgType: Send
{
    bounded_async::<MsgType>(bound)
}

#[inline] 
pub fn is_tx_closed<M> (tx: &MpscSender<M>)->bool { 
    tx.is_closed() 
}

#[inline] 
pub fn is_tx_disconnected<M> (tx: &MpscSender<M>)->bool { 
    tx.is_disconnected() 
}

#[inline] 
pub fn send<M> (tx: &MpscSender<M>, msg: M)->SendFuture<'_,M> { 
    tx.send(msg) 
}

#[inline] 
pub fn recv<M> (rx: &MpscReceiver<M>)->ReceiveFuture<'_,M> { 
    rx.recv() 
}

#[inline]
pub fn is_rx_closed<M> (rx: &MpscReceiver<M>)->bool {
    rx.is_closed()
}

#[inline]
pub fn close_rx<M> (rx: &MpscReceiver<M>)->bool {
    rx.close()
}

#[macro_export]
macro_rules! match_try_send {
    ($sender:expr, $msg:expr, ok => $ok_blk:block full => $full_blk:block closed => $closed_blk:block) => {
        match $sender.try_send($msg) {
            Ok(true) => $ok_blk
            Ok(false) => $full_blk
            Err(_) => $closed_blk
        }
    }
}
pub use match_try_send;