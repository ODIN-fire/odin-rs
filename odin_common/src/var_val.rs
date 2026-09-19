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

use crate::{common_prefix, num_type::{NumericType, accumulate_weighted_slices, average, average_slices, interpolate, interpolate_slices}};

/// something that can be either a single value or a vec of values
/// (could be extended to tuples)
#[derive(Deserialize,Clone)]
#[serde(untagged)]
pub enum VariadicVal<V> where V: Clone {
    Single(V),
    Multiple(Vec<V>)
}

impl<V:Debug+Clone> Debug for VariadicVal<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VariadicVal::Single(v) => v.fmt(f),
            VariadicVal::Multiple(v) => v.fmt(f)
        }
    }
}

impl<V:Debug+Clone> Index<usize> for VariadicVal<V> {
    type Output = V;
    fn index (&self, index: usize) -> &Self::Output {
        match self {
            VariadicVal::Single(v) => &v,
            VariadicVal::Multiple(vs) => &vs[index]
        }
    }
}

impl<'a,V:Debug+Clone> IntoIterator for &'a VariadicVal<V> {
    type Item = &'a V;
    type IntoIter = std::slice::Iter<'a, V>;

    fn into_iter(self)->Self::IntoIter {
        match self {
            VariadicVal::Single(v) => std::slice::from_ref(v).iter(),
            VariadicVal::Multiple(vs) => vs.iter()
        }
    }
}

impl <'a,V> VariadicVal<V> where V: Debug+Clone {
    pub fn is_multiple (&self)->bool {
        match self {
            VariadicVal::Single(v) => false,
            VariadicVal::Multiple(vs) => true
        }
    }

    pub fn is_single (&self)->bool {
        match self {
            VariadicVal::Single(v) => true,
            VariadicVal::Multiple(vs) => false
        }
    }

    pub fn len(&self)->usize {
        match self {
            VariadicVal::Single(v) => 1,
            VariadicVal::Multiple(vs) => vs.len()
        }
    }

    pub fn is_empty (&self)->bool {
        match self {
            VariadicVal::Single(v) => false,
            VariadicVal::Multiple(vs) => vs.is_empty()
        }
    }

    pub fn iter(&'a self)->impl Iterator<Item=&'a V> {
        self.into_iter()
    }

    pub fn as_slice (&'a self)->Option<&'a [V]> {
        match self {
            VariadicVal::Single(v) => None,
            VariadicVal::Multiple(vs) => Some(vs.as_slice())
        }
    }

    pub fn as_mut_slice (&'a mut self)->Option<&'a mut [V]> {
        match self {
            VariadicVal::Single(v) => None,
            VariadicVal::Multiple(vs) => Some(vs.as_mut_slice())
        }
    }

    pub fn first (&'a self)->Option<&'a V> {
        match self {
            VariadicVal::Single(v) => Some( &v ),
            VariadicVal::Multiple(vs) => vs.first()
        }
    }

    pub fn first_mut (&'a mut self)->Option<&'a mut V> {
        match self {
            VariadicVal::Single(v) => Some( v ),
            VariadicVal::Multiple(vs) => vs.first_mut()
        }
    }

    pub fn to_vec (&self)->Vec<V> {
        match self {
            VariadicVal::Single(v) => vec![v.clone()],
            VariadicVal::Multiple(vs) => vs.clone()
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

pub fn interpolate_variadic<V: NumericType+Debug> (a: &VariadicVal<V>, b: &VariadicVal<V>, r: f64)->VariadicVal<V> {
    if a.is_multiple() && b.is_multiple() {
        let vs = interpolate_slices( a.as_slice().unwrap(), b.as_slice().unwrap(), r); // safe to unrwap as both are multiple
        VariadicVal::Multiple(vs)

    } else if a.is_single() && b.is_single() {
        let v = interpolate( *a.first().unwrap(), *b.first().unwrap(), r); // safe to unwrap as both are single
        VariadicVal::Single(v)

    } else {
        let va = a.to_vec();
        let vb = b.to_vec();
        let vs = interpolate_slices( &va, &vb, r);
        VariadicVal::Multiple(vs)
    }
}

pub fn average_variadic<V: NumericType+Debug> (avg: &VariadicVal<V>, next: &VariadicVal<V>, n: usize)->VariadicVal<V> {
    if avg.is_multiple() && next.is_multiple() {
        let new_avg = average_slices( avg.as_slice().unwrap(), next.as_slice().unwrap(), n);
        VariadicVal::Multiple(new_avg)

    } else if avg.is_single() && next.is_single() {
        let old_avg = avg.first().unwrap();
        let next_val = next.first().unwrap();
        let new_avg = average( *old_avg, *next_val, n);
        VariadicVal::Single(new_avg)

    } else {
        let old_avg = avg.to_vec();
        let next_val = next.to_vec();
        let new_avg = average_slices( &old_avg, &next_val, n);
        VariadicVal::Multiple(new_avg)
    }
}


pub fn common_variadic_prefix<V:PartialEq+Clone+Debug> (a: &VariadicVal<V>, b: &VariadicVal<V>)->VariadicVal<V> {
    if a.is_multiple() && b.is_multiple() {
        let vs = common_prefix( a.as_slice().unwrap(), b.as_slice().unwrap()); // safe to unrwap as both are multiple
        VariadicVal::Multiple(vs)

    } else if a.is_single() && b.is_single() {
        let a = a.first().unwrap();
        let b = b.first().unwrap();
        if a == b {
            VariadicVal::Single( a.clone())
        } else {
            VariadicVal::Multiple( Vec::with_capacity(0))
        }

    } else {
        let va = a.to_vec();
        let vb = b.to_vec();
        let vs = common_prefix( &va, &vb);
        VariadicVal::Multiple(vs)
    }
}

pub fn zero_variadic<V: NumericType+Debug> (base: &VariadicVal<V>)->VariadicVal<V> {
    if base.is_single() {
        VariadicVal::Single( V::zero())
    } else {
        VariadicVal::Multiple( vec![ V::zero(); base.len()])
    }
}

pub fn idw_variadic<V: NumericType+Debug> (acc: &mut VariadicVal<V>, ds: Vec<(f64,&VariadicVal<V>)>) {
    match acc {
        VariadicVal::Single(a) => {
            for (w,d) in ds.iter() {
                match *d {
                    VariadicVal::Single(v) => { *a = V::from_f64( a.to_f64() + (w * v.to_f64())) }
                    VariadicVal::Multiple(vs) => {} // error: can't accumulate multiples into single
                }
            }
        }
        VariadicVal::Multiple(a) => {
            for (w,d) in ds.iter() {
                match *d {
                    VariadicVal::Single(v) => {} // error: can't accumulate singles into multiple
                    VariadicVal::Multiple(vs) => {
                        accumulate_weighted_slices( a, &vs, *w);
                    }
                }
            }
        }
    }
}
