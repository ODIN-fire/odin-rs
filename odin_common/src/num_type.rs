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
use std::fmt::Display;
use num::Num;

/// trait to check generic numeric type categories at runtime
/// use sparingly if type classification is required and use num::NumOps for generic operations
/// note that Num is also PartialEq + Zero + One + NumOps
pub trait NumericType: Display + Copy + Num {
    fn is_float ()->bool;
    fn to_f64(&self)->f64;
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
    fn is_signed ()->bool { false }
    fn zero_value ()->Self { 0 }
    fn one_value ()->Self { 1 }
    fn max_value ()->Self { u8::MAX }
    fn min_value ()->Self { u8::MIN }
    fn byte_size ()->usize { 1 }
}
