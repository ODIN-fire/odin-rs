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

use std::collections::HashMap;

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

