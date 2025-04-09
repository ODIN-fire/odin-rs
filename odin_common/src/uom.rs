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

use std::fmt::Debug;
use uom::si::{length::{meter,kilometer,mile,nautical_mile},f64::Length};
use serde::{Serialize,Deserialize,ser::Serializer,de::Deserializer};

pub struct LengthF64(pub Length); // otherwise we can't implement foreign traits on it

#[inline]
pub fn meters (len: f64)-> Length { Length::new::<meter>(len) }

#[inline]
pub fn kilometers (len: f64)-> Length { Length::new::<kilometer>(len) }

#[inline]
pub fn miles (len: f64)-> Length { Length::new::<mile>(len) }

#[inline]
pub fn nautical_miles (len: f64)-> Length { Length::new::<nautical_mile>(len) }

impl Debug for LengthF64 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Length({}m)", self.0.get::<meter>())
    }
}


//--- serialization support

pub fn ser_length_as_meters<S: Serializer> (length: &Length, s: S) -> Result<S::Ok, S::Error>  {
    let len: f64 = length.get::<meter>();
    s.serialize_f64(len)
}

pub fn de_length_from_meters <'a,D>(deserializer: D) -> Result<Length,D::Error> where D: Deserializer<'a> {
    let v: f64 = f64::deserialize(deserializer)?;
    Ok( Length::new::<meter>(v) )
}