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

use std::{fmt,marker::PhantomData, ops, cmp};

#[inline]
pub fn normalize_90 (d:f64) -> f64 {
    let mut x = d % 360.0;
    if x < 0.0 { x = 360.0 + x } // normalize to 0..360

    if x > 270.0 { x - 360.0}
    else if x > 90.0 { 180.0 - x }
    else { x }
}

#[inline]
pub fn normalize_180 (d: f64) -> f64 {
    let mut x = d % 360.0;
    
    if x < -180.0 { return 360.0 + x }
    if x > 180.0 { return x - 360.0 }
    x
}

#[inline]
pub fn normalize_360 (d: f64) -> f64 {
    let x = d % 360.0;
    if x < 0.0 { 360.0 + x } else { x }
}

pub trait AngleKind {
    fn normalize(v: f64)->f64;
    fn fmt_display(value: f64, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}deg", value) }
    fn fmt_debug(value: f64, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

#[derive(Debug,Clone,Copy)]
pub struct LatitudeKind {}
impl AngleKind for LatitudeKind {
    fn normalize(v: f64) -> f64 { normalize_90(v) }
    fn fmt_debug(value: f64, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "Latitude({})", value) }
}

#[derive(Debug,Clone,Copy)]
pub struct LongitudeKind {}
impl AngleKind for LongitudeKind {
    fn normalize(v: f64) -> f64 { normalize_180(v) }
    fn fmt_debug(value: f64, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "Longitude({})", value) }
}

#[derive(Copy, Clone)]
pub struct NormalizedAngle<K> where K: AngleKind {
    value: f64,
    kind: PhantomData<K>,
}

impl<K> NormalizedAngle<K> where K: AngleKind {
    #[inline]
    pub fn from_degrees(deg: f64) -> Self {
        NormalizedAngle {
            value: K::normalize(deg),
            kind: PhantomData,
        }
    }

    #[inline] pub fn radians(self)->f64 { self.value.to_radians() }
    #[inline] pub fn degrees(self)->f64 { self.value }

    // the functions that require conversion to radians
    #[inline] pub fn sin(self)->f64 { self.value.to_radians().sin() }
    #[inline] pub fn cos(self)->f64 { self.value.to_radians().cos() }
    #[inline] pub fn tan(self)->f64 { self.value.to_radians().tan() }

    #[inline] pub fn sin2(self)->f64 { self.value.to_radians().sin().powi(2) }
    #[inline] pub fn cos2(self)->f64 { self.value.to_radians().cos().powi(2) }
    #[inline] pub fn tan2(self)->f64 { self.value.to_radians().tan().powi(2) }

    #[inline] pub fn asin(self)->f64 { self.value.to_radians().sin() }
    #[inline] pub fn acos(self)->f64 { self.value.to_radians().cos() }
    #[inline] pub fn atan(self)->f64 { self.value.to_radians().atan() }
    //... and more to follow
}

//--- formatting

impl<K> fmt::Display for NormalizedAngle<K> where K: AngleKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { K::fmt_display( self.value, f) }
}

impl<K> fmt::Debug for NormalizedAngle<K> where K: AngleKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { K::fmt_debug( self.value, f) }
}

impl<K> cmp::Ord for NormalizedAngle<K> where K: AngleKind {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        if self.value < other.value { cmp::Ordering::Less }
        else if self.value == other.value { cmp::Ordering::Equal }
        else { cmp::Ordering::Greater }
    }
}

impl<K> cmp::PartialOrd for NormalizedAngle<K> where K: AngleKind {
    fn partial_cmp(&self,other:&Self) -> Option<cmp::Ordering> { Some(self.cmp(other)) }
}

impl<K> cmp::Eq for NormalizedAngle<K> where K: AngleKind { }

impl<K> cmp::PartialEq for NormalizedAngle<K> where K: AngleKind {
    fn eq(&self, other: &Self) -> bool { self.value == other.value }
}

//--- allowed num ops

// addition and subtraction is only allowed with same kind of angle
impl<K> ops::Add<NormalizedAngle<K>> for NormalizedAngle<K> where K: AngleKind {
    type Output = Self;
    fn add (self,rhs:NormalizedAngle<K>) -> Self::Output { NormalizedAngle::from_degrees( self.value + rhs.value) }
}
impl<K> ops::Sub<NormalizedAngle<K>> for NormalizedAngle<K> where K: AngleKind {
    type Output = Self;
    fn sub (self,rhs:NormalizedAngle<K>) -> Self::Output { NormalizedAngle::from_degrees( self.value - rhs.value) }
}

// multiplication and division is only allowed with floats
impl<K> ops::Mul<f64> for NormalizedAngle<K> where K: AngleKind {
    type Output = Self;
    fn mul (self,rhs:f64) -> Self::Output { NormalizedAngle::from_degrees( self.value * rhs) }
}
impl<K> ops::Div<f64> for NormalizedAngle<K> where K: AngleKind {
    type Output = Self;
    fn div (self,rhs:f64) -> Self::Output { NormalizedAngle::from_degrees( self.value / rhs) }
}

pub type Longitude = NormalizedAngle<LongitudeKind>;
pub type Latitude = NormalizedAngle<LatitudeKind>;

//--- serde support

use serde::ser::{Serialize as SerializeTrait, Serializer, SerializeStruct};
use serde::de::{self, Deserialize as DeserializeTrait, Deserializer, Visitor, SeqAccess, MapAccess};

impl<'de> DeserializeTrait<'de> for Longitude {
    fn deserialize<D>(deserializer: D) -> Result<Longitude, D::Error> where D: Deserializer<'de> {
        struct LonVisitor;

        impl<'de> Visitor<'de> for LonVisitor {
            type Value = Longitude;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("expecting floating point degrees between [-180.0..180.0] ")
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> where E: de::Error {
                use std::f64;
                if value >= -180.0 && value <= 180.0 {
                    Ok(Longitude::from_degrees(value))
                } else {
                    Err(E::custom(format!("longitude out of range: {}", value)))
                }
            }
        }

        deserializer.deserialize_f64( LonVisitor)
    }
}

impl<'de> DeserializeTrait<'de> for Latitude {
    fn deserialize<D>(deserializer: D) -> Result<Latitude, D::Error> where D: Deserializer<'de> {
        struct LatVisitor;

        impl<'de> Visitor<'de> for LatVisitor {
            type Value = Latitude;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("expecting floating point degrees between [-90.0..90.0] ")
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> where E: de::Error {
                use std::f64;
                if value >= -90.0 && value <= 90.0 {
                    Ok(Latitude::from_degrees(value))
                } else {
                    Err(E::custom(format!("latitude out of range: {}", value)))
                }
            }
        }

        deserializer.deserialize_f64( LatVisitor)
    }
}

impl<K> SerializeTrait for NormalizedAngle<K> where K: AngleKind {

    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        serializer.serialize_f64(self.value)
    }
}
