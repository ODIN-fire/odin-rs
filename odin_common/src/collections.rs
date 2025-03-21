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

use std::collections::{HashMap,VecDeque};

/// trait to get a snapshot Vec of cloned entries of the receiver collection.
/// Useful to iterate over current entries of a mutable collection
/// example:
/// ```rust
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

/// find all keys in a HashMap with given value reference
pub fn find_keys_for_value<'a,K,V>(map: &'a HashMap<K,V>, value: &V) -> Vec<&'a K> where V: PartialEq {
    map.iter()
        .filter_map(|(key, &ref val)| if *val == *value { Some(key) } else { None })
        .collect()
}

pub fn new_vec<T> ()->Vec<T> {
    Vec::new()
}

pub fn empty_vec<T> ()->Vec<T> {
    Vec::with_capacity(0)
}

/// make sure a VecDeque used as a ringbuffer (i.e. with bounded size) has space for an additional element
#[inline]
pub fn ensure_ringbuffer_space<T> (v: &mut VecDeque<T>) {
    if v.len() == v.capacity() {
        v.pop_front();
    }
}

/// push a new element to the end of a VecDeque used as a ringbuffer (i.e. in bounded space)
#[inline]
pub fn push_to_ringbuffer<T> (v: &mut VecDeque<T>, t: T) {
    ensure_ringbuffer_space(v);
    v.push_back(t)
}

/// push a new element to the end of a VecDeque used as a ringbuffer (i.e. in bounded space)
#[inline]
pub fn insert_into_ringbuffer<T> (v: &mut VecDeque<T>, idx: usize, t: T) {
    ensure_ringbuffer_space(v);
    v.insert( idx, t)
}