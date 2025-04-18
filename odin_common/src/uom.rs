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
use uom::si::{
    f64::{ThermodynamicTemperature,Length,Power}, 
    length::{kilometer, meter, mile, nautical_mile},
    thermodynamic_temperature::{kelvin},
    power::{megawatt}
};
use serde::{Serialize,Deserialize,ser::{Serializer,Error},de::Deserializer};

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

pub fn ser_length_as_rounded_meters<S: Serializer> (length: &Length, s: S) -> Result<S::Ok, S::Error>  {
    let len: i64 = length.get::<meter>().round() as i64;
    s.serialize_i64(len)
}

pub fn de_length_from_meters <'a,D>(deserializer: D) -> Result<Length,D::Error> where D: Deserializer<'a> {
    let v: f64 = f64::deserialize(deserializer)?;
    Ok( Length::new::<meter>(v) )
}

pub fn ser_temp_as_kelvin<S: Serializer> (temp: &ThermodynamicTemperature, s: S) -> Result<S::Ok, S::Error>  {
    let temp: f64 = temp.get::<kelvin>();
    s.serialize_f64(temp)
}

pub fn ser_temp_option_as_kelvin<S: Serializer> (temp: &Option<ThermodynamicTemperature>, s: S) -> Result<S::Ok, S::Error>  {
    if let Some(temp) = temp {
        let v = temp.get::<kelvin>();
        s.serialize_f64(v)
    } else {
        Err( S::Error::custom("no option value (use #[serde(skip_if=\"Option::is_none\")] field attribute)"))
    }
}

pub fn ser_temp_as_rounded_kelvin<S: Serializer> (temp: &ThermodynamicTemperature, s: S) -> Result<S::Ok, S::Error>  {
    let temp: i64 = temp.get::<kelvin>().round() as i64;
    s.serialize_i64(temp)
}

pub fn ser_temp_option_as_rounded_kelvin<S: Serializer> (temp: &Option<ThermodynamicTemperature>, s: S) -> Result<S::Ok, S::Error>  {
    if let Some(temp) = temp {
        let temp: i64 = temp.get::<kelvin>().round() as i64;
        s.serialize_i64(temp)
    } else {
        Err( S::Error::custom("no option value (use #[serde(skip_if=\"Option::is_none\")] field attribute)"))
    }
}

pub fn de_temp_from_kelvin <'a,D>(deserializer: D) -> Result<ThermodynamicTemperature,D::Error> where D: Deserializer<'a> {
    let v: f64 = f64::deserialize(deserializer)?;
    Ok( ThermodynamicTemperature::new::<kelvin>(v) )
}

pub fn de_temp_option_from_kelvin <'a,D>(deserializer: D) -> Result<Option<ThermodynamicTemperature>,D::Error> where D: Deserializer<'a> {
    let v: f64 = f64::deserialize(deserializer)?;
    Ok( Some(ThermodynamicTemperature::new::<kelvin>(v)) )
}

pub fn ser_power_as_mw<S: Serializer> (power: &Power, s: S) -> Result<S::Ok, S::Error>  {
    let power: f64 = power.get::<megawatt>();
    s.serialize_f64(power)
}

pub fn ser_power_option_as_mw<S: Serializer> (power: &Option<Power>, s: S) -> Result<S::Ok, S::Error>  {
    if let Some(power) = power {
        let v = power.get::<megawatt>();
        s.serialize_f64(v)
    } else {
        Err( S::Error::custom("no option value (use #[serde(skip_if=\"Option::is_none\")] field attribute)"))
    }
}

pub fn de_power_from_mw <'a,D>(deserializer: D) -> Result<Power,D::Error> where D: Deserializer<'a> {
    let v: f64 = f64::deserialize(deserializer)?;
    Ok( Power::new::<megawatt>(v) )
}

pub fn de_power_option_from_mw <'a,D>(deserializer: D) -> Result<Option<Power>,D::Error> where D: Deserializer<'a> {
    let v: f64 = f64::deserialize(deserializer)?;
    Ok( Some(Power::new::<megawatt>(v)) )
}