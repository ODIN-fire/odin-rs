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
use std::{fmt::Debug, collections::HashMap, hash::Hash, borrow::Borrow, path::{Path,PathBuf}, fs::File, io::{Read,BufReader}};
use serde::{Serialize,Deserialize};
use serde_json;
use async_trait::async_trait;

pub mod actor;
//pub mod share_service;

pub mod errors;


define_load_asset!{}

pub trait SharedStoreValueConstraints = Clone + Send + Sync + Debug + 'static + for<'a> Deserialize<'a> + Serialize;

/// abstraction for a general key-value store we can use as a storage mechanism for shared values
/// the main purpose is to create trait objects that provide iterator methods
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

    fn save (&self)->Result<(),OdinShareError>;
    //... possibly more to follow
}

/// an action with a KvStore trait object as execute parameter
#[async_trait]
trait SharedStoreActionTrait<T> {
    async fn execute (&self, store: &dyn SharedStore<T>) -> Result<(),OdinActionFailure>;
}

#[macro_export]
macro_rules! shared_store_action {
    ( $( let $v:ident : $v_type:ty = $v_expr:expr ),* => |$store:ident as & dyn SharedStore <$data_type:ty>| $e:expr ) => {
        {
            use async_trait::async_trait;
            use odin_share::{SharedStore,SharedStoreActionTrait};
            use odin_action::OdinActionFailure;

            struct SomeSharedStoreAction { $( $v: $v_type ),* }

            #[async_trait]
            impl SharedStoreActionTrait<$data_type> for SomeSharedStoreAction {
                async fn execute (&self, $store: &dyn SharedStore<$data_type>) -> std::result::Result<(),OdinActionFailure> {
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

#[async_trait]
pub trait DynSharedStoreActionTrait<T>: Debug + Send + Sync {
    async fn execute (&self, store: &dyn SharedStore<T>) -> Result<(),OdinActionFailure>;
}

pub type DynSharedStoreAction<T> = Box<dyn DynSharedStoreActionTrait<T>>;

#[macro_export]
macro_rules! dyn_shared_store_action {
    ( $( let $v:ident : $v_type:ty = $v_expr:expr ),* => |$store:ident as & dyn SharedStore <$data_type:ty>| $e:expr ) => {
        {
            use async_trait::async_trait;
            use odin_share::{SharedStore,DynSharedStoreActionTrait};
            use odin_action::OdinActionFailure;

            struct SomeDynSharedStoreAction { $( $v: $v_type ),* }

            #[async_trait]
            impl DynSharedStoreActionTrait<$data_type> for SomeDynSharedStoreAction {
                async fn execute (&self, $store: &dyn SharedStore<$data_type>) -> std::result::Result<(),OdinActionFailure> {
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
pub struct PersistentHashMapStore<T>{
    path: PathBuf,
    map: HashMap<String,T>
}

impl<T> PersistentHashMapStore<T> where T: SharedStoreValueConstraints {
    fn new<P> (path: &P)->Result<Self,OdinShareError> where P: AsRef<Path> {
        let map = hashmap_store_from(path)?;
        let path = path.as_ref().to_path_buf();
        Ok( PersistentHashMapStore { path, map } )
    }
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

    fn save (&self)->Result<(),OdinShareError> {
        let file = File::open(&self.path)?;
        serde_json::to_writer_pretty(file, &self.map)?;
        Ok(())
    }
}

/* endregion KvStore impls */