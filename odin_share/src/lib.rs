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
#![feature(trait_alias)]

use errors::OdinShareError;
use odin_build::prelude::*;
use odin_action::OdinActionFailure;
use std::{borrow::Borrow, collections::HashMap, fmt::{Debug, Write}, fs::File, hash::Hash, io::{BufReader, Read, Write as IOWrite}, marker::PhantomData, ops::Deref, path::{Path,PathBuf}};
use serde::{Serialize,Deserialize};
use serde_json;
use async_trait::async_trait;

pub mod prelude;
pub mod actor;
pub mod share_service;

pub mod errors;

define_load_asset!{}

pub trait SharedStoreValueConstraints = Clone + Send + Sync + Debug + 'static + Serialize + for<'a> Deserialize<'a> ;

/// abstraction for a general key-value store we can use as a storage mechanism for shared values
/// the main purpose is to create trait objects that provide iterator methods.
/// Unfortunately this also means we cannot have Serialize or Deserialize as super-traits, which means we either have
/// to use the iterators to create a vector of store items or we have to use to_json() and assemble messages that transmit
/// the store contents explicitly
#[async_trait]
pub trait SharedStore<T> : Send + Sync
    where T: SharedStoreValueConstraints
{
    fn ref_iter<'a>(&'a self)->Box<dyn Iterator<Item=(&'a String,&'a T)> + 'a>;
    
    fn clone_iter(&self)->Box<dyn Iterator<Item=(String,T)> + '_> {
        Box::new( self.ref_iter().map( |(ref_k,ref_v)| (ref_k.clone(), ref_v.clone())))
    }

    /// this is here so that disk-based stores don't have to iterate over all keys
    fn glob_ref_iter<'a>(&'a self, glob_pattern: &str)->Result<Box<dyn Iterator<Item=(&'a String,&'a T)> + 'a>, OdinShareError>;
    fn glob_clone_iter(&self, glob_pattern: &str)->Result<Box<dyn Iterator<Item=(String,T)> + '_>, OdinShareError>;

    fn len(&self)->usize;
    fn contains_key (&self, k: &str)->bool;
    fn insert(&mut self, k: String, v: T)->Option<T>;
    fn remove (&mut self, k: &str)->Option<T>;
    fn get (&self, k: &str)->Option<&T>;

    fn to_json (&self)->Result<String,OdinShareError>;
    fn save (&self)->Result<(),OdinShareError>;

    // override if store isn't initialized upon construction
    async fn initialize (&self)->Result<(),OdinShareError> { 
        Ok(()) // initialized upon construction
    }

    //... possibly more to follow
}

/// an action with a SharedStore trait object as execute argument
#[async_trait]
pub trait SharedStoreAction<T> {
    async fn execute (&self, store: &dyn SharedStore<T>) -> Result<(),OdinActionFailure>;
    fn is_empty (&self)->bool { false }
}

#[macro_export]
macro_rules! shared_store_action {
    ( $( let $v:ident : $v_type:ty = $v_expr:expr ),* => |$store:ident as & dyn SharedStore <$data_type:ty>| $e:expr ) => {
        {
            struct SomeSharedStoreAction { $( $v: $v_type ),* }

            #[async_trait::async_trait]
            impl $crate::SharedStoreAction<$data_type> for SomeSharedStoreAction {
                async fn execute (&self, $store: &dyn $crate::SharedStore<$data_type>) -> std::result::Result<(),odin_action::OdinActionFailure> {
                    $( let $v = &self. $v;)*
                    $e
                }
            }
            impl std::fmt::Debug for SomeSharedStoreAction {
                fn fmt (&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "SharedStoreAction<{}>", stringify!($data_type))
                }
            }

            SomeSharedStoreAction{ $( $v: $v_expr),* }
        }
    }
}

/// helper for empty action
pub struct NoSharedStoreAction<T> where T: Send { _phantom: PhantomData<T> }
#[async_trait]
impl<T> SharedStoreAction<T> for NoSharedStoreAction<T> where T: Send + Sync {
    async fn execute (&self, _store: &dyn SharedStore<T>) -> Result<(),OdinActionFailure> { Ok(()) }
    fn is_empty (&self) -> bool { true }
}
impl<T> Debug for NoSharedStoreAction<T> where T: Send {
    fn fmt (&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NoSharedStoreAction<{}>", std::any::type_name::<T>())
    }
}
pub fn no_shared_store_action<T>()->NoSharedStoreAction<T> where T: Send { NoSharedStoreAction { _phantom: PhantomData } }


#[async_trait]
pub trait DynSharedStoreActionTrait<T>: Debug + Send + Sync {
    async fn execute (&self, store: &dyn SharedStore<T>) -> Result<(),OdinActionFailure>;
}

pub type DynSharedStoreAction<T> = Box<dyn DynSharedStoreActionTrait<T>>;

#[macro_export]
macro_rules! dyn_shared_store_action {
    ( $( let $v:ident : $v_type:ty = $v_expr:expr ),* => |$store:ident as & dyn SharedStore <$data_type:ty>| $e:expr ) => {
        {
            struct SomeDynSharedStoreAction { $( $v: $v_type ),* }

            #[async_trait::async_trait]
            impl $crate::DynSharedStoreActionTrait<$data_type> for SomeDynSharedStoreAction {
                async fn execute (&self, $store: &dyn $crate::SharedStore<$data_type>) -> std::result::Result<(),odin_action::OdinActionFailure> {
                    $( let $v = &self. $v;)*
                    $e
                }
            }
            impl std::fmt::Debug for SomeDynSharedStoreAction {
                fn fmt (&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "SharedStoreAction<{}>", stringify!($data_type))
                }
            }

            Box::new( SomeDynSharedStoreAction{ $( $v: $v_expr),* } )
        }
    }
}

/* #region KvStore impls **************************************************************************/

impl<T> SharedStore<T> for HashMap<String,T>
    where T: SharedStoreValueConstraints
{
    fn ref_iter<'a>(&'a self)->Box<dyn Iterator<Item=(&'a String,&'a T)> + 'a> {
        Box::new( self.iter())
    }

    fn glob_ref_iter<'a> (&'a self, glob_pattern: &str)->Result<Box<dyn Iterator<Item=(&'a String,&'a T)> + 'a>, OdinShareError> {
        let glob = globset::Glob::new(glob_pattern)?.compile_matcher();
        Ok( Box::new( self.iter().filter( move |(k,v)| glob.is_match(k) )) )
    }

    fn glob_clone_iter(&self, glob_pattern: &str)->Result<Box<dyn Iterator<Item=(String,T)> + '_>, OdinShareError> {
        let glob = globset::Glob::new(glob_pattern)?.compile_matcher();
        Ok( Box::new( self.iter().filter( move |(k,v)| glob.is_match(k) ).map( |(ref_k,ref_v)| (ref_k.clone(),ref_v.clone())) ) )
    }

    fn len(&self)->usize { 
        HashMap::len(self) 
    }

    fn contains_key (&self, k: &str)->bool { 
        HashMap::contains_key(self, k) 
    }

    fn insert(&mut self, k: String, v: T)->Option<T> {
        HashMap::insert(self, k, v)
    }

    fn remove (&mut self, k: &str)->Option<T> {
        HashMap::remove(self, k)
    }

    fn get (&self, k: &str)->Option<&T> {
        HashMap::get(self,k)
    }

    fn to_json (&self)->Result<String,OdinShareError> {
        Ok( serde_json::to_string( &self)? )
    }

     // this is an in-memory only map - we don't save anything
    fn save (&self)->Result<(),OdinShareError> { Ok(()) }
}

/// creates a HashMap SharedStore from path to JSON file.
/// Note this only initializes the HashMap but doesn't store it
pub fn hashmap_store_from<P,T> (path: &P)->Result<HashMap<String,T>, OdinShareError> where P: AsRef<Path>, T: SharedStoreValueConstraints {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let map: HashMap<String,T> = serde_json::from_reader(reader)?;
    Ok( map )
}

/// a HashMap-based SharedStore that is both initialized from and saved to given JSON path
#[derive(Serialize)]
pub struct PersistentHashMapStore<T>{
    #[serde(skip,default="default_store_path")]
    path: PathBuf,
    map: HashMap<String,T>
}

fn default_store_path()->PathBuf {
    Path::new("shared_store.json").to_path_buf()
}

impl<T> PersistentHashMapStore<T> where T: SharedStoreValueConstraints {
    fn new<P> (path: &P)->Result<Self,OdinShareError> where P: AsRef<Path> {
        let map = hashmap_store_from(path)?;
        let path = path.as_ref().to_path_buf();
        Ok( PersistentHashMapStore { path, map } )
    }
}

impl<T> Deref for PersistentHashMapStore<T> {
    type Target = HashMap<String,T>;
    fn deref(&self) -> &Self::Target { &self.map }
}

impl<T> SharedStore<T> for PersistentHashMapStore<T>
    where T: SharedStoreValueConstraints
{
    fn ref_iter<'a>(&'a self)->Box<dyn Iterator<Item=(&'a String,&'a T)> + 'a> {
        Box::new( self.map.iter())
    }

    fn glob_ref_iter<'a> (&'a self, glob_pattern: &str)->Result<Box<dyn Iterator<Item=(&'a String,&'a T)> + 'a>, OdinShareError> {
        let glob = globset::Glob::new(glob_pattern)?.compile_matcher();
        Ok( Box::new( self.map.iter().filter( move |(k,v)| glob.is_match(k) )) )
    }

    fn glob_clone_iter(&self, glob_pattern: &str)->Result<Box<dyn Iterator<Item=(String,T)> + '_>, OdinShareError> {
        let glob = globset::Glob::new(glob_pattern)?.compile_matcher();
        Ok( Box::new( self.map.iter().filter( move |(k,v)| glob.is_match(k) ).map( |(ref_k,ref_v)| (ref_k.clone(),ref_v.clone())) ) )
    }

    fn len(&self)->usize { 
        self.map.len() 
    }

    fn contains_key (&self, k: &str)->bool { 
        self.map.contains_key(k) 
    }

    fn insert(&mut self, k: String, v: T)->Option<T> {
        self.map.insert( k, v)
    }

    fn remove (&mut self, k: &str)->Option<T> {
        self.map.remove(k)
    }

    fn get (&self, k: &str)->Option<&T> {
        self.map.get(k)
    }

    fn to_json (&self)->Result<String,OdinShareError> {
        Ok( serde_json::to_string( &self.map)? )
    }

    fn save (&self)->Result<(),OdinShareError> {
        let file = File::open(&self.path)?;
        serde_json::to_writer_pretty(file, &self.map)?;
        Ok(())
    }
}

/* endregion KvStore impls */