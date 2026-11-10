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
#![allow(unused_macros)]
#![doc = include_str!("../doc/odin_action.md")]

// TODO - re-evaluate if we need our own type now that async closures are stabilized.
// having all these specialized action types requiring special macro syntax is sub-optimal.
// This is currently still required because of DynAction - async closures are not dyn-compatible

use std::{fmt::Debug, future::{Future,ready}, result::Result, pin::Pin,
    any::type_name, ops::{Deref,DerefMut},
};
pub use async_trait::async_trait;

/// return only the last part of a type path
pub fn abbrev_type_name<T>()->String {
    let full_name = type_name::<T>();

    match full_name.rfind(':') {
        None => full_name.to_string(),
        Some(idx) => unsafe { full_name.get_unchecked(idx+1..).to_string() }
    }
}

/// wrapper type for action `Result` errors.
/// We do need a single `Result` error type for actions so that we can have homogenous collections of Action instances
/// of the same argument type. 
/// We also want to support the ? operator in execute() bodies without having to explicitly `map_err(..)` all error types 
/// we can encounter. 
/// 
/// Enforcing the same generic error type for all actions in such a collection would be too restrictive unless we can have
/// a blanket `impl From<T>` for our error type. Unfortunately we can't have `impl std::error::Error for OdinActionFailure`
/// and a `impl<T:Error> From<T> for OdinActionFailure` at the same time so `OdinActionFailure` does **not** 
/// impl `std::error::Error`
pub struct OdinActionFailure(pub String);

#[macro_export]
macro_rules! map_action_failure {
    ($error_enum:ty, $error_variant:ident) => {
        impl From<odin_action::OdinActionFailure> for $error_enum {
            fn from (e: odin_action::OdinActionFailure)->$error_enum { <$error_enum>::$error_variant( e.to_string()) }
        }
    }
}

impl<T> From<T> for OdinActionFailure where T: std::fmt::Display {
    fn from (e:T)->Self {
        OdinActionFailure(e.to_string())
    }
}

impl std::fmt::Debug for OdinActionFailure {
    fn fmt (&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OdinActionError(\"{}\")", self.0)
    }
}

impl ToString for OdinActionFailure {
    fn to_string(&self)->String { format!("{:?}", self) }
}

/*
impl std::fmt::Display for OdinActionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "action failed: {}", self.0)
    }
}
*/


/* #region DataAction ************************************************************************/

/// a trait that includes a single `execute(&self,T)` method returning a future.
/// This is used as a type constraint for types that represent parameterized async actions taking
/// a single data argument.
///
/// Actions that take a reference to data owned by the executing actor are `DataAction<&'a T>` for any lifetime `'a`,
/// i.e. they are constrained by `for<'a> DataAction<&'a T>` or the equivalent [`DataRefAction<T>`] alias
pub trait DataAction<T>: Debug + Send {
    fn execute (&self, data: T) -> impl Future<Output = Result<(),OdinActionFailure>> + Send;
    fn is_empty (&self)->bool { false }
}

/// macro to define and instantiate ad hoc action types that clone-capture local vars and take a single
/// `execute(data)` argument. See [crate] doc for general use and expansion.
/// If the data argument is a reference (`|data: &T|`) the action implements `DataAction<&'a T>` for all lifetimes `'a`
/// (and hence [`DataRefAction<T>`])
#[macro_export]
macro_rules! data_action {
    ( $( let $v:ident : $v_type:ty = $v_expr:expr ),* => |$data:ident : & $data_type:ty| $e:expr ) => {
        {
            struct SomeDataAction { $( $v: $v_type ),* }

            impl<'a> $crate::DataAction<&'a $data_type> for SomeDataAction {
                async fn execute (&self, $data : &'a $data_type) -> std::result::Result<(),$crate::OdinActionFailure> {
                    $( let $v = &self. $v;)*
                    $e
                }
            }
            impl std::fmt::Debug for SomeDataAction {
                fn fmt (&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "DataAction<&{}>", stringify!($data_type))
                }
            }

            SomeDataAction{ $( $v: $v_expr ),* }
        }
    };
    ( $( let $v:ident : $v_type:ty = $v_expr:expr ),* => |$data:ident : $data_type:ty| $e:expr ) => {
        {
            struct SomeDataAction { $( $v: $v_type ),* }

            impl $crate::DataAction<$data_type> for SomeDataAction {
                async fn execute (&self, $data : $data_type) -> std::result::Result<(),$crate::OdinActionFailure> {
                    $( let $v = &self. $v;)*
                    $e
                }
            }
            impl std::fmt::Debug for SomeDataAction {
                fn fmt (&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "DataAction<{}>", stringify!($data_type))
                }
            }

            SomeDataAction{ $( $v: $v_expr ),* }
        }
    };
}

/// the empty action, which implements all (non-dyn) action traits for all data types. Transparent alternative for
/// `Option<..Action>` (see the `no_..action()` functions)
#[derive(Debug,Clone,Copy,Default)]
pub struct NoAction;

impl NoAction {
    /// inherent version so that calls on concrete `NoAction` values are not ambiguous between action traits
    pub fn is_empty (&self)->bool { true }
}

impl<T> DataAction<T> for NoAction {
    fn execute (&self, _data: T) -> impl Future<Output = Result<(),OdinActionFailure>> + Send { ready(Ok(())) }
    fn is_empty (&self)->bool { true }
}

pub fn no_data_action()->NoAction { NoAction }

/* #endregion DataAction */

/* #region DataMutAction ******************************************************************************/

/// a `DataAction` that can mutate its state (not the passed in data)
pub trait DataMutAction<T>: Debug + Send {
    fn execute (&mut self, data: T) -> impl Future<Output = Result<(),OdinActionFailure>> + Send;
    fn is_empty (&self)->bool { false }
}

/// macro to define and instantiate ad hoc action types that capture mutable local vars and take a single
/// `execute(data)` argument. See [crate] doc for general use and expansion.
#[macro_export]
macro_rules! data_mut_action {
    ( $( let mut $v:ident : $v_type:ty = $v_expr:expr ),* => |$data:ident : & $data_type:ty| $e:expr ) => {
        {
            struct SomeDataMutAction { $( $v: $v_type ),* }

            impl<'a> $crate::DataMutAction<&'a $data_type> for SomeDataMutAction {
                async fn execute (&mut self, $data : &'a $data_type) -> std::result::Result<(),$crate::OdinActionFailure> {
                    $( let $v = &mut self. $v;)*
                    $e
                }
            }
            impl std::fmt::Debug for SomeDataMutAction {
                fn fmt (&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "DataMutAction<&{}>", stringify!($data_type))
                }
            }

            SomeDataMutAction{ $( $v: $v_expr ),* }
        }
    };
    ( $( let mut $v:ident : $v_type:ty = $v_expr:expr ),* => |$data:ident : $data_type:ty| $e:expr ) => {
        {
            struct SomeDataMutAction { $( $v: $v_type ),* }

            impl $crate::DataMutAction<$data_type> for SomeDataMutAction {
                async fn execute (&mut self, $data : $data_type) -> std::result::Result<(),$crate::OdinActionFailure> {
                    $( let $v = &mut self. $v;)*
                    $e
                }
            }
            impl std::fmt::Debug for SomeDataMutAction {
                fn fmt (&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "DataMutAction<{}>", stringify!($data_type))
                }
            }

            SomeDataMutAction{ $( $v: $v_expr ),* }
        }
    };
}

impl<T> DataMutAction<T> for NoAction {
    fn execute (&mut self, _data: T) -> impl Future<Output = Result<(),OdinActionFailure>> + Send { ready(Ok(())) }
    fn is_empty (&self)->bool { true }
}

/* #endregion DataMutAction */

/* #region BiDataAction ******************************************************************************/

/// a [`DataAction<T>`] with an `async execute(..)` function that takes a second `bidata` parameter.
/// This can be used for actions that combine owned and passed-in data in their action bodies.
///
/// Note there is no corresponding `BiDynDataAction` as this normally would be a [`DynDataAction`]
/// that captures the bidata. `BiDataAction` is a way to avoid the associated runtime cost of dyn actions
/// if requester and actor both know the bidata type and the requester can pass it in through a message.
pub trait BiDataAction<T,A>: Debug + Send {
    fn execute (&self, data: T, bidata: A) -> impl Future<Output = Result<(),OdinActionFailure>> + Send;
}

/// macro to define and instantiate ad hoc actions taking two data arguments (of which the first can be a reference).
/// See [crate] doc for general use and expansion.
#[macro_export]
macro_rules! bi_data_action {
    ( $( let $v:ident : $v_type:ty = $v_expr:expr ),* => |$data:ident : & $data_type:ty, $bidata:ident : $bidata_type:ty| $e:expr ) => {
        {
            struct SomeBiDataAction { $( $v: $v_type ),* }

            impl<'a> $crate::BiDataAction<&'a $data_type,$bidata_type> for SomeBiDataAction {
                async fn execute (&self, $data : &'a $data_type, $bidata : $bidata_type) -> std::result::Result<(),$crate::OdinActionFailure> {
                    $( let $v = &self. $v;)*
                    $e
                }
            }
            impl std::fmt::Debug for SomeBiDataAction {
                fn fmt (&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "BiDataAction<&{},{}>", stringify!($data_type), stringify!($bidata_type))
                }
            }

            SomeBiDataAction{ $( $v: $v_expr ),* }
        }
    };
    ( $( let $v:ident : $v_type:ty = $v_expr:expr ),* => |$data:ident : $data_type:ty, $bidata:ident : $bidata_type:ty| $e:expr ) => {
        {
            struct SomeBiDataAction { $( $v: $v_type ),* }

            impl $crate::BiDataAction<$data_type,$bidata_type> for SomeBiDataAction {
                async fn execute (&self, $data : $data_type, $bidata : $bidata_type) -> std::result::Result<(),$crate::OdinActionFailure> {
                    $( let $v = &self. $v;)*
                    $e
                }
            }
            impl std::fmt::Debug for SomeBiDataAction {
                fn fmt (&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "BiDataAction<{},{}>", stringify!($data_type), stringify!($bidata_type))
                }
            }

            SomeBiDataAction{ $( $v: $v_expr ),* }
        }
    };
}

impl<T,A> BiDataAction<T,A> for NoAction {
    fn execute (&self, _data: T, _bidata: A) -> impl Future<Output = Result<(),OdinActionFailure>> + Send { ready(Ok(())) }
}

pub fn no_bi_data_action()->NoAction { NoAction }

/* #endregion BiDataAction */

/* #region DynDataAction *****************************************************************************************/

/// the boxed future returned by dyn action `execute(..)` calls. It can borrow from both the action and the data
pub type ActionFuture<'s> = Pin<Box<dyn Future<Output = Result<(),OdinActionFailure>> + Send + 's>>;

/// a sendable [`DataAction<T>`] that can be stored in a homogenous container (as respective trait objects).
/// This trait is mostly used implicitly through the corresponding [`DynDataAction<T>`] type.
/// Note: this has per-execution runtime cost as the returned future needs to be pinboxed.
///
/// We do not use `async_trait` here since it cannot express futures that borrow reference data arguments
/// (`DynDataActionTrait<&'a T>`, see [`DynDataRefAction<T>`])
pub trait DynDataActionTrait<T>: Debug + Send + Sync {
    fn execute<'s> (&'s self, data: T) -> ActionFuture<'s> where T: 's;
}

/// a type alias for a boxed [`DynDataActionTrait<T>`] trait object, used to send and store respective actions.
pub type DynDataAction<T> = Box<dyn DynDataActionTrait<T>>;

/// macro to define and instantiate ad hoc [`DynDataAction<T>`] types (or [`DynDataRefAction<T>`] types if the data
/// argument is a reference). See [crate] doc for general use and expansion.
/// To be used where actions have to be send and/or stored in homogenous containers (as trait objects)
#[macro_export]
macro_rules! dyn_data_action {
    ( $( let $v:ident : $v_type:ty = $v_expr:expr ),* => |$data:ident : & $data_type:ty| $e:expr ) => {
        {
            struct SomeDynDataAction { $( $v: $v_type ),* }

            impl SomeDynDataAction {
                async fn exec<'a> (&self, $data : &'a $data_type) -> std::result::Result<(),$crate::OdinActionFailure> {
                    $( let $v = &self. $v;)*
                    $e
                }
            }
            impl<'a> $crate::DynDataActionTrait<&'a $data_type> for SomeDynDataAction {
                fn execute<'s> (&'s self, $data : &'a $data_type) -> $crate::ActionFuture<'s> where &'a $data_type: 's {
                    Box::pin( self.exec( $data))
                }
            }
            impl std::fmt::Debug for SomeDynDataAction {
                fn fmt (&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "DynDataAction<&{}>", stringify!($data_type))
                }
            }

            Box::new( SomeDynDataAction{ $( $v: $v_expr ),* })
        }
    };
    ( $( let $v:ident : $v_type:ty = $v_expr:expr ),* => |$data:ident : $data_type:ty| $e:expr ) => {
        {
            struct SomeDynDataAction { $( $v: $v_type ),* }

            impl SomeDynDataAction {
                async fn exec (&self, $data : $data_type) -> std::result::Result<(),$crate::OdinActionFailure> {
                    $( let $v = &self. $v;)*
                    $e
                }
            }
            impl $crate::DynDataActionTrait<$data_type> for SomeDynDataAction {
                fn execute<'s> (&'s self, $data : $data_type) -> $crate::ActionFuture<'s> where $data_type: 's {
                    Box::pin( self.exec( $data))
                }
            }
            impl std::fmt::Debug for SomeDynDataAction {
                fn fmt (&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "DynDataAction<{}>", stringify!($data_type))
                }
            }

            Box::new( SomeDynDataAction{ $( $v: $v_expr ),* })
        }
    };
}

/* #endregion DynDataAction */

/* #region reference action aliases ********************************************************************************/

// Actions that take references to data owned by the executing actor are just the respective by-value actions for
// reference types with arbitrary lifetimes. The following traits and types are aliases for those (higher ranked)
// trait bounds, and the `..ref..` macros are aliases for the respective by-value macros with a reference argument.
// They are kept for brevity and backwards compatibility.

/// alias for `for<'a> DataAction<&'a T>`, i.e. an action that takes a reference argument with arbitrary lifetime.
/// Note this is automatically implemented for all respective `DataAction` types - it cannot be implemented explicitly
pub trait DataRefAction<T: ?Sized>: for<'a> DataAction<&'a T> {}
impl<T: ?Sized, X> DataRefAction<T> for X where X: for<'a> DataAction<&'a T> {}

/// alias for [`data_action`] with a reference argument (`|data: &T|`)
#[macro_export]
macro_rules! dataref_action {
    ( $( let $v:ident : $v_type:ty = $v_expr:expr ),* => |$data:ident : & $data_type:ty| $e:expr ) => {
        $crate::data_action!( $( let $v : $v_type = $v_expr ),* => |$data : & $data_type| $e )
    }
}

pub fn no_dataref_action()->NoAction { NoAction }

/// alias for `for<'a> DataMutAction<&'a T>`
pub trait DataRefMutAction<T: ?Sized>: for<'a> DataMutAction<&'a T> {}
impl<T: ?Sized, X> DataRefMutAction<T> for X where X: for<'a> DataMutAction<&'a T> {}

/// alias for [`data_mut_action`] with a reference argument (`|data: &T|`)
#[macro_export]
macro_rules! dataref_mut_action {
    ( $( let mut $v:ident : $v_type:ty = $v_expr:expr ),* => |$data:ident : & $data_type:ty| $e:expr ) => {
        $crate::data_mut_action!( $( let mut $v : $v_type = $v_expr ),* => |$data : & $data_type| $e )
    }
}

/// alias for `for<'a> BiDataAction<&'a T,A>`
pub trait BiDataRefAction<T: ?Sized,A>: for<'a> BiDataAction<&'a T,A> {}
impl<T: ?Sized, A, X> BiDataRefAction<T,A> for X where X: for<'a> BiDataAction<&'a T,A> {}

/// alias for [`bi_data_action`] with a reference argument (`|data: &T, bidata: A|`)
#[macro_export]
macro_rules! bi_dataref_action {
    ( $( let $v:ident : $v_type:ty = $v_expr:expr ),* => |$data:ident : & $data_type:ty, $bidata:ident : $bidata_type:ty| $e:expr ) => {
        $crate::bi_data_action!( $( let $v : $v_type = $v_expr ),* => |$data : & $data_type, $bidata : $bidata_type| $e )
    }
}

pub fn no_bi_dataref_action()->NoAction { NoAction }

/// a boxed [`DynDataActionTrait<&'a T>`] trait object for arbitrary lifetimes `'a`
/// Note: this has per-execution runtime cost as the returned future needs to be pinboxed
pub type DynDataRefAction<T> = Box<dyn for<'a> DynDataActionTrait<&'a T>>;

/// alias for [`dyn_data_action`] with a reference argument (`|data: &T|`)
#[macro_export]
macro_rules! dyn_dataref_action {
    ( $( let $v:ident : $v_type:ty = $v_expr:expr ),* => |$data:ident : & $data_type:ty| $e:expr ) => {
        $crate::dyn_data_action!( $( let $v : $v_type = $v_expr ),* => |$data : & $data_type| $e )
    }
}

/* #endregion reference action aliases */

/* #region dyn action lists *********************************************************************************/

/// container to store DynDataAction objects
pub struct DynDataActionList<T> where T: Clone {
    entries: Vec<DynDataAction<T>> 
}

impl <T> DynDataActionList<T> where T: Clone {
    pub fn new ()->Self { 
        DynDataActionList{ entries: Vec::new() } 
    }
    
    pub async fn execute (&self, data: T, ignore_err: bool) -> Result<(),OdinActionFailure> {
        if !self.is_empty() {
            let n = self.entries.len()-1;
            if ignore_err {
                for i in 0..n { let _ = self.entries[i].execute(data.clone()).await; }
                let _ = self.entries[n].execute(data).await;
            } else {
                for i in 0..n { self.entries[i].execute(data.clone()).await?; }
                self.entries[n].execute(data).await?;
            }
        }
        Ok(())
    }
}

impl <T> Deref for DynDataActionList<T> where T: Clone {
    type Target = Vec<DynDataAction<T>>;
    fn deref(& self) -> &Self::Target {
        &self.entries
    }
}

impl <T> DerefMut for DynDataActionList<T> where T: Clone {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.entries
    }
}


/// container to store DynDataRefAction objects
pub struct DynDataRefActionList<T> where T: Send + Sync {
    entries: Vec<DynDataRefAction<T>> 
}

impl <T> DynDataRefActionList<T> where T: Send + Sync {
    pub fn new ()->Self { 
        DynDataRefActionList{ entries: Vec::new() }
    }

    pub async fn execute (&self, data: &T, ignore_err: bool) -> Result<(),OdinActionFailure> {
        if ignore_err {
            for e in &self.entries {
                let _ = e.execute(data).await;
            }
        } else {
            for e in &self.entries {
                e.execute(data).await?;
            }
        }
        Ok(())
    }
}

impl <T> Deref for DynDataRefActionList<T> where T: Send + Sync {
    type Target = Vec<DynDataRefAction<T>>;
    fn deref(& self) -> &Self::Target {
        &self.entries
    }
}

impl <T> DerefMut for DynDataRefActionList<T> where T: Send + Sync {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.entries
    }
}

/* #endregion dyn action lists */