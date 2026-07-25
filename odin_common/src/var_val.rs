/*
 * Copyright 2026 Peter Mehlitz
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://apache.org
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use std::{fmt::{Debug,Formatter}, ops::{Deref,Index}};
use serde::{Deserialize};

/// something that can be either a single value or a vec of values
/// (could be extended to tuples)
#[derive(Deserialize)]
#[serde(untagged)]
pub enum VariadicVal<V> {
    Single(V),
    Multiple(Vec<V>)
}

impl<V:Debug> Debug for VariadicVal<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VariadicVal::Single(v) => v.fmt(f),
            VariadicVal::Multiple(v) => v.fmt(f)
        }
    }
}

impl<V> Index<usize> for VariadicVal<V> {
    type Output = V;
    fn index (&self, index: usize) -> &Self::Output {
        match self {
            VariadicVal::Single(v) => &v,
            VariadicVal::Multiple(vs) => &vs[index]
        }
    }
}

impl<'a,V> IntoIterator for &'a VariadicVal<V> {
    type Item = &'a V;
    type IntoIter = std::slice::Iter<'a, V>;

    fn into_iter(self)->Self::IntoIter {
        match self {
            VariadicVal::Single(v) => std::slice::from_ref(v).iter(),
            VariadicVal::Multiple(vs) => vs.iter()
        }
    }
}

impl <'a,V> VariadicVal<V> where V: Clone {
    pub fn len(&self)->usize {
        match self {
            VariadicVal::Single(v) => 1,
            VariadicVal::Multiple(vs) => vs.len()
        }
    }

    pub fn iter(&'a self)->impl Iterator<Item=&'a V> {
        self.into_iter()
    }

    pub fn is_empty (&self)->bool {
        match self {
            VariadicVal::Single(v) => false,
            VariadicVal::Multiple(vs) => vs.is_empty()
        }
    }

    pub fn push (&mut self, new_val: V) {
        match self {
            VariadicVal::Single(v) => {
                *self = VariadicVal::Multiple( vec![v.clone(), new_val]);
            }
            VariadicVal::Multiple(vs) => {
                vs.push( new_val);
            }
        }
    }

    //... and possibly more to make it Vec compatible
}
