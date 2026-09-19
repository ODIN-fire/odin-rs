/*
 * Copyright © 2026, United States Government, as represented by the Administrator of
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
use std::{fmt::Display,num::{NonZero}};
use num::Num;
use crate::define_error;

/// trait to check generic numeric type categories at runtime
/// use sparingly if type classification is required and use num::NumOps for generic operations
/// note that Num is also PartialEq + Zero + One + NumOps
/// NOTE - the from_f64() call might clamp to the target type boundaries
pub trait NumericType: Display + Copy + Num {
    fn is_float ()->bool;
    fn to_f64(&self)->f64;
    /// note this might clamp to Self MIN,MAX values
    fn from_f64(v:f64)->Self;
    fn is_integer ()->bool { !Self::is_float() }
    fn is_signed ()->bool;
    fn is_unsigned ()->bool { !Self::is_signed() }
    fn zero_value ()->Self;
    fn one_value ()->Self;
    fn max_value ()->Self;
    fn min_value ()->Self;
    fn byte_size ()->usize;
}

impl NumericType for f64 {
    fn is_float ()->bool { true }
    fn to_f64(&self)->f64 { *self }
    fn from_f64(v:f64)->Self { v }
    fn is_signed ()->bool { true }
    fn zero_value ()->Self { 0.0 }
    fn one_value ()->Self { 1.0 }
    fn max_value ()->Self { f64::MAX }
    fn min_value ()->Self { f64::MIN }
    fn byte_size ()->usize { 8 }
}

impl NumericType for f32 {
    fn is_float ()->bool { true }
    fn to_f64(&self)->f64 { *self as f64 }
    fn from_f64(v:f64)->Self { v as f32 }
    fn is_signed ()->bool { true }
    fn zero_value ()->Self { 0.0 }
    fn one_value ()->Self { 1.0 }
    fn max_value ()->Self { f32::MAX }
    fn min_value ()->Self { f32::MIN }
    fn byte_size ()->usize { 4 }
}

impl NumericType for i64 {
    fn is_float ()->bool { false }
    fn to_f64(&self)->f64 { *self as f64 }
    fn from_f64(v:f64)->Self { v as i64 }
    fn is_signed ()->bool { true }
    fn zero_value ()->Self { 0 }
    fn one_value ()->Self { 1 }
    fn max_value ()->Self { i64::MAX }
    fn min_value ()->Self { i64::MIN }
    fn byte_size ()->usize { 8 }
}

impl NumericType for i32 {
    fn is_float ()->bool { false }
    fn to_f64(&self)->f64 { *self as f64 }
    fn from_f64(v:f64)->Self { v as i32 }
    fn is_signed ()->bool { true }
    fn zero_value ()->Self { 0 }
    fn one_value ()->Self { 1 }
    fn max_value ()->Self { i32::MAX }
    fn min_value ()->Self { i32::MIN }
    fn byte_size ()->usize { 4 }
}

impl NumericType for i16 {
    fn is_float ()->bool { false }
    fn to_f64(&self)->f64 { *self as f64 }
    fn from_f64(v:f64)->Self { v as i16 }
    fn is_signed ()->bool { true }
    fn zero_value ()->Self { 0 }
    fn one_value ()->Self { 1 }
    fn max_value ()->Self { i16::MAX }
    fn min_value ()->Self { i16::MIN }
    fn byte_size ()->usize { 2 }
}

impl NumericType for i8 {
    fn is_float ()->bool { false }
    fn to_f64(&self)->f64 { *self as f64 }
    fn from_f64(v:f64)->Self { v as i8 }
    fn is_signed ()->bool { true }
    fn zero_value ()->Self { 0 }
    fn one_value ()->Self { 1 }
    fn max_value ()->Self { i8::MAX }
    fn min_value ()->Self { i8::MIN }
    fn byte_size ()->usize { 1 }
}

impl NumericType for u64 {
    fn is_float ()->bool { false }
    fn to_f64(&self)->f64 { *self as f64 }
    fn from_f64(v:f64)->Self { v as u64 }
    fn is_signed ()->bool { false }
    fn zero_value ()->Self { 0 }
    fn one_value ()->Self { 1 }
    fn max_value ()->Self { u64::MAX }
    fn min_value ()->Self { u64::MIN }
    fn byte_size ()->usize { 8 }
}

impl NumericType for u32 {
    fn is_float ()->bool { false }
    fn to_f64(&self)->f64 { *self as f64 }
    fn from_f64(v:f64)->Self { v as u32 }
    fn is_signed ()->bool { false }
    fn zero_value ()->Self { 0 }
    fn one_value ()->Self { 1 }
    fn max_value ()->Self { u32::MAX }
    fn min_value ()->Self { u32::MIN }
    fn byte_size ()->usize { 4 }
}

impl NumericType for u16 {
    fn is_float ()->bool { false }
    fn to_f64(&self)->f64 { *self as f64 }
    fn from_f64(v:f64)->Self { v as u16 }
    fn is_signed ()->bool { false }
    fn zero_value ()->Self { 0 }
    fn one_value ()->Self { 1 }
    fn max_value ()->Self { u16::MAX }
    fn min_value ()->Self { u16::MIN }
    fn byte_size ()->usize { 2 }
}

impl NumericType for u8 {
    fn is_float ()->bool { false }
    fn to_f64(&self)->f64 { *self as f64 }
    fn from_f64(v:f64)->Self { v as u8 }
    fn is_signed ()->bool { false }
    fn zero_value ()->Self { 0 }
    fn one_value ()->Self { 1 }
    fn max_value ()->Self { u8::MAX }
    fn min_value ()->Self { u8::MIN }
    fn byte_size ()->usize { 1 }
}

/// linearly interpolate two slices of NumericType instances
pub fn interpolate_slices<T: NumericType> (a: &[T], b: &[T], r: f64)->Vec<T> {
    a.iter().zip( b.iter()).map( |(a,b)| {
        interpolate( *a, *b, r)
    }).collect()
}

#[inline]
pub fn interpolate<T: NumericType> (a: T, b: T, r: f64)->T {
    let v0 = a.to_f64();
    let v1 = b.to_f64();

    let v = v0 + (v1 - v0)*r;
    T::from_f64(v)
}

pub fn mut_average<T: NumericType> (avg: &mut [T], next: &[T], n: usize) {
    let len = avg.len().min(next.len());
    for i in 0..len {
        avg[i] = T::from_f64( (next[i].to_f64() - avg[i].to_f64())/(n as f64))
    }
}

/// caller has to handle first element explicitly and keep track of element count
pub fn average_slices<T: NumericType> (last_avg: &[T], next: &[T], n: usize)->Vec<T> {
    last_avg.iter().zip( next.iter()).map( |(last_avg, next)| {
        average( *last_avg, *next, n)
    }).collect()
}

/// caller has to handle first element explicitly and keep track of element count
#[inline]
pub fn average<T: NumericType> (last_avg: T, next: T, n: usize)->T {
    last_avg + T::from_f64( (next.to_f64() - last_avg.to_f64())/(n as f64))
}

pub fn accumulate_weighted<T: NumericType> (acc: &T, next: &T, w: f64)->T {
    T::from_f64( acc.to_f64() + next.to_f64() * w)
}

// note this is mutating since we want to avoid per-accumulation allocation
pub fn accumulate_weighted_slices<T: NumericType> (acc: &mut [T], next: &[T], w: f64) {
    for i in 0..acc.len() {
        acc[i] = accumulate_weighted( &acc[i], &next[i], w);
    }
}

define_error!{ pub OdinNumError =
    NonZeroError() : "NonZero error"
}

// note that std::num::ZeroablePrimitive is internal and permanently unstable so we can't provide generic functions
// (f32 and f64 do not implement ZeroablePrimtive)

pub fn non_zero_u8  (n: u8) ->Result<NonZero<u8>,OdinNumError>  { NonZero::<u8>::new(n).ok_or( OdinNumError::NonZeroError()) }
pub fn non_zero_u16 (n: u16)->Result<NonZero<u16>,OdinNumError> { NonZero::<u16>::new(n).ok_or( OdinNumError::NonZeroError()) }
pub fn non_zero_u32 (n: u32)->Result<NonZero<u32>,OdinNumError> { NonZero::<u32>::new(n).ok_or( OdinNumError::NonZeroError()) }
pub fn non_zero_u64 (n: u64)->Result<NonZero<u64>,OdinNumError> { NonZero::<u64>::new(n).ok_or( OdinNumError::NonZeroError()) }
pub fn non_zero_usize (n: usize)->Result<NonZero<usize>,OdinNumError> { NonZero::<usize>::new(n).ok_or( OdinNumError::NonZeroError()) }

pub fn non_zero_i8  (n: i8) ->Result<NonZero<i8>,OdinNumError>  { NonZero::<i8>::new(n).ok_or( OdinNumError::NonZeroError()) }
pub fn non_zero_i16 (n: i16)->Result<NonZero<i16>,OdinNumError> { NonZero::<i16>::new(n).ok_or( OdinNumError::NonZeroError()) }
pub fn non_zero_i32 (n: i32)->Result<NonZero<i32>,OdinNumError> { NonZero::<i32>::new(n).ok_or( OdinNumError::NonZeroError()) }
pub fn non_zero_i64 (n: i64)->Result<NonZero<i64>,OdinNumError> { NonZero::<i64>::new(n).ok_or( OdinNumError::NonZeroError()) }
pub fn non_zero_isize (n: isize)->Result<NonZero<isize>,OdinNumError> { NonZero::<isize>::new(n).ok_or( OdinNumError::NonZeroError()) }
