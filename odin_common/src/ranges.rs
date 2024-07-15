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

use std::ops::{Add,Mul};
use std::fmt::{Debug,Display};

/* #region LinearRange **********************************************************************************/

pub trait RangeTypeTrait = Debug + Display + Copy + Add<Output=Self> + Mul<Output=Self> + UsizeOps;

/// represents a bounded linear range of values with a fixed increment and an element type that
/// supports addition and multiplication. Supports conversion into stand alone Iterator.
#[derive(Debug,Clone)]
pub struct LinearRange<T> where T: RangeTypeTrait {
    first: T,
    inc: T,
    n: usize
}

impl <T> LinearRange<T> where T: RangeTypeTrait {

    pub fn new (first: T, inc: T, n: usize)->Self {
        //if n == 0 { panic!("empty range") } 
        LinearRange{first,inc,n}
    }

    pub fn from_bounds(first: T, last: T, n: usize)->Self {
        LinearRange{first, inc: last.div_usize(n), n}
    }

    #[inline] pub fn at (&self, idx:usize)->T {
        if idx >= self.n { panic!("index {} out of bounds 0..{}", idx, self.n) }
        self.first + self.inc.mul_usize(idx)
    }

    #[inline] pub fn first (&self)->T { self.first }

    #[inline] pub fn last (&self)->T { self.first + self.inc.mul_usize(self.n) }

    #[inline] pub fn inc (&self)->T { self.inc }

    #[inline] pub fn len (&self)->usize { self.n }

    pub fn as_iter  (&self)->LinearRangeIterator<T> {
        LinearRangeIterator{src: self.clone(), idx: 0}
    }
}

/// iterator for LinearRange that captures its source
pub struct LinearRangeIterator <T> where T: RangeTypeTrait {
    src: LinearRange<T>,
    idx: usize
}

impl<T> Iterator for LinearRangeIterator <T> where T: RangeTypeTrait {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        let src = &self.src;
        let idx = self.idx;
        if idx < src.n { 
            self.idx += 1;
            Some( src.first + src.inc.mul_usize(idx) )
        } else {
            None
        }
    }
}

impl<T> IntoIterator for LinearRange<T> where T: RangeTypeTrait {
    type Item = T;
    type IntoIter = LinearRangeIterator<T>;

    fn into_iter(self) -> Self::IntoIter {
        LinearRangeIterator{src:self,idx:0}
    }
} 

/* #endregion LinearRange */

/* #region UsizeOps *****************************************************************************/

/// multiplication and division with RHS=usize
/// Note this uses primitive type cast and hence does panic if the usize value cannot be converted into Self
pub trait UsizeOps { 
    fn mul_usize (&self, n: usize)->Self;
    fn div_usize (&self, n: usize)->Self;
}

impl UsizeOps for f64 {  
    fn mul_usize(&self, n: usize)->f64 { *self * (n as f64) } 
    fn div_usize(&self, n: usize)->f64 { *self / (n as f64) } 
}
impl UsizeOps for f32 {  
    fn mul_usize(&self, n: usize)->f32 { *self * (n as f32) } 
    fn div_usize(&self, n: usize)->f32 { *self / (n as f32) } 
}
impl UsizeOps for u64 {  
    fn mul_usize(&self, n: usize)->u64 { *self * (n as u64) } 
    fn div_usize(&self, n: usize)->u64 { *self / (n as u64) } 
}
impl UsizeOps for i64 {  
    fn mul_usize(&self, n: usize)->i64 { *self * (n as i64) } 
    fn div_usize(&self, n: usize)->i64 { *self / (n as i64) } 
}
impl UsizeOps for u32 {  
    fn mul_usize(&self, n: usize)->u32 { *self * (n as u32) } 
    fn div_usize(&self, n: usize)->u32 { *self / (n as u32) } 
}
impl UsizeOps for i32 {  
    fn mul_usize(&self, n: usize)->i32 { *self * (n as i32) } 
    fn div_usize(&self, n: usize)->i32 { *self / (n as i32) } 
}
impl UsizeOps for u16 {  
    fn mul_usize(&self, n: usize)->u16 { *self * (n as u16) } 
    fn div_usize(&self, n: usize)->u16 { *self / (n as u16) } 
}
impl UsizeOps for i16 {  
    fn mul_usize(&self, n: usize)->i16 { *self * (n as i16) } 
    fn div_usize(&self, n: usize)->i16 { *self / (n as i16) } 
}
impl UsizeOps for u8  {  
    fn mul_usize(&self, n: usize)->u8  { *self * (n as u8) } 
    fn div_usize(&self, n: usize)->u8  { *self / (n as u8) } 
}
impl UsizeOps for i8  {  
    fn mul_usize(&self, n: usize)->i8  { *self * (n as i8) } 
    fn div_usize(&self, n: usize)->i8  { *self / (n as i8) } 
}