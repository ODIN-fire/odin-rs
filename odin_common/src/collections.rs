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

use std::error::Error;
use std::result::Result;
use std::hash::{BuildHasher,Hash};
use std::collections::{HashMap,VecDeque};

use hashbrown::hash_map::{HashMap as HbHashMap, RawEntryMut};


pub fn new_vec<T> ()->Vec<T> {
    Vec::new()
}

pub fn empty_vec<T> ()->Vec<T> {
    Vec::with_capacity(0)
}

/// a collection that can be turned into a Vec of item references
pub trait RefVec<T> {
    fn as_ref_vec (&self) -> Vec<&T>;
}

impl<T> RefVec<T> for Vec<T> {
    fn as_ref_vec (&self) -> Vec<&T> {
        self.iter().map( |e| e).collect()
    }
}

impl<T> RefVec<T> for VecDeque<T> {
    fn as_ref_vec (&self) -> Vec<&T> {
        self.iter().map( |e| e).collect()
    }
}


pub trait SortedCollection<T> {
    fn sort_in<F> (&mut self, t: T, is_before: F) where F: Fn(&T,&T)->bool;
} 

impl <T> SortedCollection<T> for Vec<T> {
    fn sort_in<F> (&mut self, t: T, is_before: F) where F: Fn(&T,&T)->bool {
        if !self.is_empty() {
            if is_before( self.last().unwrap(), &t) { // shortcut for sorted entry - can't panic since collection not empty
                self.push(t);
                return;
            } 

            for (i,c) in self.iter().enumerate() {
                if is_before( &t, &self[i]) {
                    self.insert( i, t);
                    return;
                }
            }
            unreachable!(); // see shortcut above

        } else { // first item
            self.push( t);
        }
    }
} 

/* #region deque (ring buffer) *****************************************************************************************/

/// VecDeque extension trait to use a VecDeque as a ringbuffer (i.e. constant space) 
pub trait RingDeque<T> {
    fn new (capacity: usize)->Self;
    fn is_full (&self)->bool;
    fn push_to_ringbuffer (&mut self, t: T);
    fn insert_into_ringbuffer (&mut self, idx: usize, t: T);

    fn sort_into_ringbuffer<F> (&mut self, t: T, is_before: F) where F: Fn(&T,&T)->bool;
}

impl <T> RingDeque<T> for VecDeque<T> {
    fn new (capacity: usize)->Self {
        VecDeque::with_capacity( capacity)
    }

    #[inline]
    fn is_full (&self)->bool {
        self.len() == self.capacity()
    }

    fn push_to_ringbuffer (&mut self, t: T) {
        if self.len() == self.capacity() { self.pop_front(); }
        self.push_back(t)
    }

    fn insert_into_ringbuffer (&mut self, idx: usize, t: T) {
        if self.len() == self.capacity() { self.pop_front(); }
        self.insert( idx, t)
    }

    fn sort_into_ringbuffer<F> (&mut self, t: T, is_before: F) where F: Fn(&T,&T)->bool {
        if !self.is_empty() {
            if is_before( self.back().unwrap(), &t) { // shortcut for sorted entry - can't panic since collection not empty
                self.push_to_ringbuffer(t);
                return;
            } 

            for (i,c) in self.iter().enumerate() {
                if is_before( &t, &self[i]) {
                    self.insert_into_ringbuffer( i, t);
                    return;
                }
            }
            unreachable!(); // see shortcut above

        } else { // first item
            self.push_back(t);
        }
    }
}

/* #endregion deque */

/* #region sorted iterable lookup **********************************************************************************************/

/// given a (sorted) item iterator and a distance function return a reference to the closest item
/// the distance can be positive or negative
/// Note this is O(N) in case the diff of the last element is positive - specialize if the collection supports O(1) element lookup
/// Note also the caller has to make sure the impl is actually sorted
pub fn find_closest_from_sorted_iter<'a,T,I,F> (mut it: I, f: F)->Option<&'a T> where I: Iterator<Item=&'a T>, F: Fn(&T)->f64 {
    let mut last_diff: f64 = f64::NAN;
    let mut last_item: Option<&T> = None;

    if let Some(item) = it.next() {
        last_diff = f(item);
        last_item = Some(item);
        if last_diff < 0.0 { return last_item; }  // first item is to the right so it is closest
    }
    
    while let Some(item) = it.next() {
        let d = f(item);
        if d < 0.0 {
            if last_diff > -d { return Some(item); } else { return last_item; }
        }
        last_diff = d;
        last_item = Some(item);
    }

    last_item  // last item is to the left
}

pub trait SortedIterable<T> {
    fn find_closest<'a,F> (&'a self, f: F)->Option<&'a T> where T: 'a, F: Fn(&T)->f64;
}

impl <T> SortedIterable<T> for Vec<T> {
    fn find_closest<'a,F> (&'a self, f: F)->Option<&'a T> where T: 'a, F: Fn(&T)->f64 {
        if let Some(last) = self.last() {
            if f(last) > 0.0 { 
                return Some(last); 
            } else {
                find_closest_from_sorted_iter(self.iter(), f)
            }
        } else {
            None
        }
    }
}

impl <T> SortedIterable<T> for VecDeque<T> {
    fn find_closest<'a,F> (&'a self, f: F)->Option<&'a T> where T: 'a, F: Fn(&T)->f64 {
        if let Some(last) = self.back() {
            if f(last) > 0.0 { 
                return Some(last); 
            } else {
                find_closest_from_sorted_iter(self.iter(), f)
            }
        } else {
            None
        }
    }
}

/* #endregion sorted iterable lookup */

/* #region  hashmap ***************************************************************************************************/

/// HashMap extension that adds a function to insert an item if it isn't in the hashmap yet.
/// This is an optimization in case hashing is expensive and we want to save per-operation
/// lookup cost (e.g. for potentially large HashMaps that are populated on-demand)
pub trait SingleLookupHashMap<K,V> {

    /// check if we already have an item for the provided key. If not, use the provided closure to
    /// enter a new key/value pair. Since the value constructor is infallible this always returns a value reference 
    fn get_or_insert<'a,F> (&'a mut self,  key: &'a K, f: F) -> &'a V 
        where K: Eq + Hash + Clone, F: FnOnce()->V;

    /// version for fallible value constructors
    fn get_or_try_insert<'a,F> (&'a mut self,  key: &'a K, f: F) -> Option<&'a V> 
        where K: Eq + Hash + Clone, F: FnOnce()->Option<V>;
}


/// hashbrown HashMap implementation that avoid double hashing when adding a non-existent entry
impl<K,V> SingleLookupHashMap<K,V> for HbHashMap<K,V> where K: Eq+Hash+Clone {

    fn get_or_insert<'a,F> (&'a mut self,  key: &'a K, f: F) -> &'a V 
        where F: FnOnce()->V
    {
        self.raw_entry_mut().from_key(key).or_insert(key.clone(), f()).1
    }

    fn get_or_try_insert<'a,F> (&'a mut self,  key: &'a K, f: F) -> Option<&'a V> 
        where F: FnOnce()->Option<V>
    {
        if let Some(value) = f() {
            Some(self.raw_entry_mut().from_key(key).or_insert(key.clone(), value).1)
        } else {
            None
        }
    }
}

/// standard HashMap implementation (which is hashbrown except of the unstable API).
/// Note that we don't use the unstable std HashMap API as it is flagged as "not planned"
/// which means this will incur at least 2 lookups per call
impl<K,V> SingleLookupHashMap<K,V> for HashMap<K,V> where K: Eq+Hash+Clone {

    fn get_or_insert<'a,F> (&'a mut self,  key: &'a K, f: F) -> &'a V 
        where F: FnOnce()->V
    {
        if !self.contains_key(key) {
            self.insert( key.clone(), f());
        } 
        self.get(key).unwrap()
    }

    fn get_or_try_insert<'a,F> (&'a mut self,  key: &'a K, f: F) -> Option<&'a V> 
        where F: FnOnce()->Option<V>
    {
        if !self.contains_key(key) {
            if let Some(value) = f() {
                self.insert( key.clone(), value);
            } else {
                return None;
            }
        } 
        self.get(key)
    }
}

/// trait to get a snapshot Vec of cloned entries of the receiver collection.
/// Useful to iterate over current entries of a mutable collection
/// example:
/// ```
/// use std::collections::HashMap;
/// use odin_common::collections::Snapshot;
/// fn foo (map: &mut HashMap<String,String>) {
///    for (k,v) in map.snapshot() {
///        map.insert( k.to_uppercase(), v.to_uppercase());
///    }
/// }
/// ```
pub trait Snapshot<E> {
    fn snapshot(&self)->Vec<E>;
}

impl<K,V> Snapshot<(K,V)> for HashMap<K,V> where K:Clone, V:Clone {
    fn snapshot(&self)->Vec<(K,V)> {
        self.iter().fold( Vec::with_capacity(self.len()), |mut acc,e| {
            acc.push( (e.0.clone(), e.1.clone())); 
            acc 
        })
    }
}

// TODO - avoid code duplication
impl<K,V> Snapshot<(K,V)> for HbHashMap<K,V> where K:Clone, V:Clone {
    fn snapshot(&self)->Vec<(K,V)> {
        self.iter().fold( Vec::with_capacity(self.len()), |mut acc,e| {
            acc.push( (e.0.clone(), e.1.clone())); 
            acc 
        })
    }
}

/// HashMap that supports value->key (reverse) lookup
pub trait ReverseLookupHashMap<K,V> {
    fn find_keys_for_value<'a>(&'a self, value: &V) -> Vec<&'a K>;
}

impl <K,V> ReverseLookupHashMap<K,V> for HashMap<K,V> where V: PartialEq {
    fn find_keys_for_value<'a>(&'a self, value: &V) -> Vec<&'a K> {
        self.iter()
            .filter_map(|(key, &ref val)| if *val == *value { Some(key) } else { None })
            .collect()
    }
}

// TODO - avoid code duplication
impl <K,V> ReverseLookupHashMap<K,V> for HbHashMap<K,V> where V: PartialEq {
    fn find_keys_for_value<'a>(&'a self, value: &V) -> Vec<&'a K> {
        self.iter()
            .filter_map(|(key, &ref val)| if *val == *value { Some(key) } else { None })
            .collect()
    }
}


/* #endegion hashmap */