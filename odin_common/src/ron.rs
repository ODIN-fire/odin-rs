/*
 * Copyright © 2025, United States Government, as represented by the Administrator of 
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

/// module with utility functions for RON serialization/deserialization
/// useful for ODIN-to-ODIN messages

use lazy_static::lazy_static;
use ron::{ser::PrettyConfig, error::Result};
use serde::{Serialize,Deserialize};
use crate::type_base_name;

lazy_static! {
    /// a pretty config that includes struct name but strips all newlines, indents and space separators
    /// useful to transmit RON serialized ODIN types
    static ref TYPED_COMPACT_RON: PrettyConfig = new_typed_compact_ron_opts();
}

pub fn new_typed_compact_ron_opts()->PrettyConfig {
    PrettyConfig::new()
        .struct_names(true)
        .compact_structs(true)
        .compact_maps(true)
        .compact_arrays(true)
        .separator("")    
}

/// used to speed up ron::ser::to_string_pretty(..) which consumes a PrettyConfig
pub fn typed_compact_ron_opts()->PrettyConfig {
    TYPED_COMPACT_RON.clone()
}

pub fn to_typed_compact_ron<T> (v: &T)->Result<String> where T: Serialize {
    ron::ser::to_string_pretty( v, typed_compact_ron_opts())
}

/// answer if input could be a RON serialization of type T, which means it starts with the type base_name followed by a a braced value
pub fn is_maybe_type<'a,T> (s: &str)->bool where T: Deserialize<'a> {
    let tn = type_base_name::<T>();
    if !s.starts_with( tn) { return false }
    
    let bs = s.as_bytes();
    let l = tn.len();
    let bl = bs.len();

    if bl <= l+1 { return false }

    match bs[l] {
        b'(' => bs[bl-1] == b')',
        b'[' => bs[bl-1] == b']',
        b'{' => bs[bl-1] == b'}',
        _ => false
    }
}

/// try to instantiate a type T from given input str
/// Does not enter the expensive parsing unless the input satisfies `is_maybe_type(s)`
/// Note this does not return a Result as there could actually be different types with the same base_name (RON does not serialize type paths)
pub fn from_typed_compact_ron <'a,T> (s: &'a str)-> Option<T> where T: Deserialize<'a> {
    if is_maybe_type::<T>(s) {
        ron::from_str(s).ok()
    } else {
        None
    }
}

/// a trait that provides default implementations for compact typed RON serialization/deserialization
pub trait TypedCompactRon<'a> where Self: Serialize + Deserialize<'a> {
    fn to_typed_compact_ron (&self)->ron::error::Result<String> { 
        to_typed_compact_ron( &self) 
    }
    fn try_from_typed_compact_ron (s: &'a str)->Option<Self> { 
        from_typed_compact_ron::<Self>(s) 
    }
}