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

use std::{
    error::Error,
    collections::HashMap,
    sync::{RwLock,LazyLock},
    path::Path, 
    ops::{Deref,DerefMut},
    fs::File, io::BufReader,
    io,
    fmt::Debug
};
use globset::GlobMatcher;
use serde::{Serialize,Deserialize};
use serde_json;
use ron;
use crate::fs;

pub trait SharedStoreValueConstraints = Clone + Debug + Serialize + for<'a> Deserialize<'a> + 'static;

pub type SharedStoreLock<T> = LazyLock<RwLock<SharedStore<T>>>;

pub const fn init_shared_store<T: SharedStoreValueConstraints> () -> SharedStoreLock<T> {
    LazyLock::new(|| RwLock::new(SharedStore::new()))
} 

#[macro_export]
macro_rules! define_shared_store {
    ($vis:vis $name:ident < $value_type:ty >) => {
        $vis static $name : odin_common::shared_store::SharedStoreLock<$value_type> = odin_common::shared_store::init_shared_store();
    }
}

/// global, RwLock-synchronized and typed key-value store with structured, path-like String keys. Implementation (e.g. in-memory HashMap) is
/// hidden behind the interface.
/// Since a SharedStore instance is supposed to be global we have to synchronize all access, which also means that we cannot return
/// borrowed values. Getters have to either return clones (hence the Clone constraint on T) or use closures to process
/// &T values that match. The interface hides the RwLock.
/// SharedStore instances can be initialized from and stored to files containing JSON data for HashMaps / objects.
/// 
/// use like so:
/// ```rust
/// use odin_common::{define_shared_store, shared_store::SharedStore};
/// 
/// define_shared_store! { pub POINT_STORE<Point2d> }
/// ...
///    SharedStore::set_from_json_file( &POINT_STORE, "my_point2d_values.json").expect("error initializing POINT2D from file");
/// ...
///    SharedStore::insert( &POINT_STORE, "/foo/bar".into(), Point2d(42.0,-42.0));
/// ...
///    SharedStore::with( &POINT_STORE, "/foo/bar", |p| println!("value of key '/foo/bar' is {p:?}"));
/// ```

pub struct SharedStore<T> {
    map: HashMap<String,T>
}

impl <T> SharedStore<T> where T: SharedStoreValueConstraints {
    pub fn new()->Self { 
        SharedStore{ map: HashMap::new()}
    }

    pub fn from_path<P: AsRef<Path>> (path: &P) -> Result<Self, Box<dyn Error>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);    
        let map: HashMap<String,T> = serde_json::from_reader(reader)?;

        Ok( SharedStore{ map } )
    }
    
    //--- the public interface, which only contains associated functions

    pub fn set_from_file<P: AsRef<Path>> (shared_store: &SharedStoreLock<T>, path: &P)->Result<(), Box<dyn Error>> {
        let file = File::open(path)?;
        if path.as_ref().is_file() {
            let reader = BufReader::new(file);    
            let map: HashMap<String,T> = match fs::extension(path) {
                Some("ron") => ron::de::from_reader(reader)?,
                Some("json") => serde_json::from_reader(reader)?,
                _ => Err( io::Error::new(io::ErrorKind::Other, "unsupported file format"))?
            };

            if !map.is_empty() {
                let mut store = shared_store.write().unwrap();
                for (k,v) in map {
                    store.insert(k, v);
                }
            }
        }

        Ok(())
    }

    pub fn write_to_json_file<P: AsRef<Path>> (shared_store: &SharedStoreLock<T>, path: &P)->Result<(), Box<dyn Error>> {
        let store = shared_store.read().unwrap();
        let mut file = File::create(path)?;
        Ok( serde_json::to_writer_pretty( file, &store.map)? )
    }

    pub fn contains_key (shared_store: &SharedStoreLock<T>, key: &str)->bool {
        let store = shared_store.read().unwrap();
        store.contains_key(key)
    }

    pub fn get_clone (shared_store: &SharedStoreLock<T>, key: &str)->Option<T> {
        let store = shared_store.read().unwrap();
        store.get(key).map(|r| r.clone() )
    }
    
    pub fn insert (shared_store: &SharedStoreLock<T>, key: String, value: T) {
        let mut store = shared_store.write().unwrap();
        store.insert( key, value);
    }
    
    pub fn remove (shared_store: &SharedStoreLock<T>, key: &str)->bool {
        let mut store = shared_store.write().unwrap();
        store.remove(key).is_some()
    }

    /// execute closure for value reference of given key
    /// Note - if f(&T) panics the lock will be poisened and all successive access to the store will panic
    pub fn with<F,R> (shared_store: &SharedStoreLock<T>, key: &str, mut f: F)->Option<R> where F: FnMut(&T)->R {
        let store = shared_store.read().unwrap();
        store.get(key).map(|r| f(r) )
    }

    pub fn get_clones_matching (shared_store: &SharedStoreLock<T>, glob: GlobMatcher)->Vec<T> {
        let mut res: Vec<T> = Vec::new();
        let store = shared_store.read().unwrap();

        for (key,value) in store.map.iter() {
            if glob.is_match( key) {
                res.push( value.clone())
            }
        }
        res
    }

    /// execute closure for all value references with keys that match a given glob
    /// Note - if f(&T) panics the lock will be poisened and all successive access to the store will panic
    pub fn with_matching <F> (shared_store: &SharedStoreLock<T>, glob: GlobMatcher, mut f: F) where F: FnMut(&T)->() {
        let store = shared_store.read().unwrap();
        for (key,value) in store.map.iter() {
            if glob.is_match( key) {
                f(value);
            }
        }
    }

    pub fn with_all <F> (shared_store: &SharedStoreLock<T>, mut f: F) where F: FnMut(&str,&T)->() {
        let store = shared_store.read().unwrap();
        for (k,v) in store.map.iter() {
            f( k.as_str(), v);
        }
    }
}

impl<T> Deref for SharedStore<T> {
    type Target = HashMap<String,T>;
    fn deref(&self) -> &Self::Target { &self.map }
}

impl<T> DerefMut for SharedStore<T> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.map }
}