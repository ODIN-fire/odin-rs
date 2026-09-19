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
use uom::{Conversion};
use uom::si::{
    Quantity, Dimension, SI,
    absement::meter_second,
    f64::{Angle, Area, HeatFluxDensity, Length, Power, Pressure, Ratio, ThermodynamicTemperature, Velocity},
    heat_flux_density::{self,watt_per_square_meter},
    length::{self,centimeter, foot, inch, kilometer, meter, mile, millimeter, nautical_mile},
    area::{self,square_meter,square_foot,square_inch,square_kilometer,hectare,square_mile,acre},
    power::{self,megawatt,watt},
    pressure::{self,hectopascal, inch_of_mercury, pascal},
    ratio::{self,percent,ratio as ratio_unit},
    thermodynamic_temperature::{self,degree_celsius, degree_fahrenheit, kelvin},
    velocity::{self,kilometer_per_hour, meter_per_second, mile_per_hour},
    angle::{self,degree,radian}
};
pub use uom::ConstZero;
use num::Num;
use serde::{Serialize,Deserialize,ser::{Serializer,Error},de::Deserializer};
use crate::{accumulate_weighted_f32,accumulate_weighted_f64, average_f64, average_f32, interpolate_f64, interpolate_f32};

//--- f64 quantity type aliases
pub type AngleF64 = uom::si::f64::Angle;
pub type RatioF64 = uom::si::f64::Ratio;
pub type LengthF64 = uom::si::f64::Length;
pub type AreaF64 = uom::si::f64::Area;
pub type PowerF64 = uom::si::f64::Power;
pub type PressureF64 = uom::si::f64::Pressure;
pub type VelocityF64 = uom::si::f64::Velocity;
pub type ThermodynamicTemperatureF64 = uom::si::f64::ThermodynamicTemperature;
pub type HeatFluxDensityF64 = uom::si::f64::HeatFluxDensity;
//... and more to follow

//--- f32 quantity type aliases
pub type AngleF32 = uom::si::f32::Angle;
pub type RatioF32 = uom::si::f32::Ratio;
pub type LengthF32 = uom::si::f32::Length;
pub type AreaF32 = uom::si::f32::Area;
pub type PowerF32 = uom::si::f32::Power;
pub type PressureF32 = uom::si::f32::Pressure;
pub type VelocityF32 = uom::si::f32::Velocity;
pub type ThermodynamicTemperatureF32 = uom::si::f32::ThermodynamicTemperature;
pub type HeatFluxDensityF32 = uom::si::f32::HeatFluxDensity;
//... and more to follow

#[inline]
pub fn meters (len: f64)-> Length { Length::new::<meter>(len) }

#[inline]
pub fn kilometers (len: f64)-> Length { Length::new::<kilometer>(len) }

#[inline]
pub fn miles (len: f64)-> Length { Length::new::<mile>(len) }

#[inline]
pub fn nautical_miles (len: f64)-> Length { Length::new::<nautical_mile>(len) }


//--- generic conversion between primitive types

/// Generic conversion from an f32-backed Quantity of any kind (Length,
/// Mass, Time, ...) to the equivalent f64-backed Quantity.
pub trait ToF64Quantity<D> where D: Dimension + ?Sized + 'static {
    fn to_f64(self) -> Quantity<D, SI<f64>, f64>;
}

impl<D> ToF64Quantity<D> for Quantity<D, SI<f32>, f32> where D: Dimension + ?Sized + 'static {
    fn to_f64(self) -> Quantity<D, SI<f64>, f64> {
        Quantity {
            dimension: std::marker::PhantomData,
            units: std::marker::PhantomData,
            value: self.value as f64,
        }
    }
}

/// Generic conversion from an f64-backed Quantity of any kind (Length,
/// Mass, Time, ...) to the equivalent f32-backed Quantity.
pub trait ToF32Quantity<D> where D: Dimension + ?Sized {
    fn to_f32(self) -> Quantity<D, SI<f32>, f32>;
}

impl<D> ToF32Quantity<D> for Quantity<D, SI<f64>, f64> where D: Dimension + ?Sized {
    fn to_f32(self) -> Quantity<D, SI<f32>, f32> {
        Quantity {
            dimension: std::marker::PhantomData,
            units: std::marker::PhantomData,
            value: self.value as f32,
        }
    }
}

//--- vector conversion (note we can't directly implement NumericType or Num traits since this defeats the purpose of UOM )

pub fn length_f64_to_vec<U> (vs: &[Length])->Vec<f64> where U: length::Unit + Conversion<f64, T = f64> { vs.iter().map(|v| v.get::<U>()).collect() }
pub fn length_f32_to_vec<U> (vs: &[LengthF32])->Vec<f32> where U: length::Unit + Conversion<f32, T = f32> { vs.iter().map(|v| v.get::<U>()).collect() }

pub fn area_f64_to_vec<U> (vs: &[Area])->Vec<f64> where U: area::Unit + Conversion<f64, T = f64> { vs.iter().map(|v| v.get::<U>()).collect() }
pub fn area_f32_to_vec<U> (vs: &[AreaF32])->Vec<f32> where U: area::Unit + Conversion<f32, T = f32> { vs.iter().map(|v| v.get::<U>()).collect() }

pub fn velocity_f64_to_vec<U> (vs: &[Velocity])->Vec<f64> where U: velocity::Unit + Conversion<f64, T = f64> { vs.iter().map(|v| v.get::<U>()).collect() }
pub fn velocity_f32_to_vec<U> (vs: &[VelocityF32])->Vec<f32> where U: velocity::Unit + Conversion<f32, T = f32> { vs.iter().map(|v| v.get::<U>()).collect() }

pub fn temperature_f64_to_vec<U> (vs: &[ThermodynamicTemperature])->Vec<f64> where U: thermodynamic_temperature::Unit + Conversion<f64, T = f64> { vs.iter().map(|v| v.get::<U>()).collect() }
pub fn temperature_f32_to_vec<U> (vs: &[ThermodynamicTemperatureF32])->Vec<f32> where U: thermodynamic_temperature::Unit + Conversion<f32, T = f32> { vs.iter().map(|v| v.get::<U>()).collect() }

pub fn pressure_f64_to_vec<U> (vs: &[Pressure])->Vec<f64> where U: pressure::Unit + Conversion<f64, T = f64> { vs.iter().map(|v| v.get::<U>()).collect() }
pub fn pressure_f32_to_vec<U> (vs: &[PressureF32])->Vec<f32> where U: pressure::Unit + Conversion<f32, T = f32> { vs.iter().map(|v| v.get::<U>()).collect() }

pub fn heatfluxdensity_f64_to_vec<U> (vs: &[HeatFluxDensity])->Vec<f64> where U: heat_flux_density::Unit + Conversion<f64, T = f64> { vs.iter().map(|v| v.get::<U>()).collect() }
pub fn heatfluxdensity_f32_to_vec<U> (vs: &[HeatFluxDensityF32])->Vec<f32> where U: heat_flux_density::Unit + Conversion<f32, T = f32> { vs.iter().map(|v| v.get::<U>()).collect() }

pub fn angle_f64_to_vec<U> (vs: &[Angle])->Vec<f64> where U: angle::Unit + Conversion<f64, T = f64> { vs.iter().map(|v| v.get::<U>()).collect() }
pub fn angle_f32_to_vec<U> (vs: &[AngleF32])->Vec<f32> where U: angle::Unit + Conversion<f32, T = f32> { vs.iter().map(|v| v.get::<U>()).collect() }

pub fn ratio_f64_to_vec<U> (vs: &[Ratio])->Vec<f64>  where U: ratio::Unit + Conversion<f64, T = f64> { vs.iter().map(|v| v.get::<U>()).collect() }
pub fn ratio_f32_to_vec<U> (vs: &[RatioF32])->Vec<f32> where U: ratio::Unit + Conversion<f32, T = f32> { vs.iter().map(|v| v.get::<U>()).collect() }

//... and more to follow

//--- IDW support

// note we do not need to convert to/from units as we can perform the operations in respective transparent base units

macro_rules! define_accumulate_weighted {
    ($fname:ident, $qtype:ty) => {
        pub fn $fname (acc: &mut [$qtype], next: &[$qtype], w: f64) {
            for i in 0..acc.len() { acc[i].value = accumulate_weighted_f64( &acc[i].value, &next[i].value, w); }
        }
    };
    ($fname:ident, $qtype:ty, f32) => {
        pub fn $fname (acc: &mut [$qtype], next: &[$qtype], w: f64) {
            for i in 0..acc.len() { acc[i].value = accumulate_weighted_f32( &acc[i].value, &next[i].value, w as f32); }
        }
    };
}

define_accumulate_weighted!( accumulate_weighted_length, Length);
define_accumulate_weighted!( accumulate_weighted_area, Area);
define_accumulate_weighted!( accumulate_weighted_angle, Angle);
define_accumulate_weighted!( accumulate_weighted_ratio, Ratio);
define_accumulate_weighted!( accumulate_weighted_thermodynamic_temperature, ThermodynamicTemperature);
define_accumulate_weighted!( accumulate_weighted_velocity, Velocity);
define_accumulate_weighted!( accumulate_weighted_pressure, Pressure);
define_accumulate_weighted!( accumulate_weighted_heat_flux_density, HeatFluxDensity);

define_accumulate_weighted!( accumulate_weighted_length_f32, LengthF32, f32);
define_accumulate_weighted!( accumulate_weighted_area_f32, AreaF32, f32);
define_accumulate_weighted!( accumulate_weighted_angle_f32, AngleF32, f32);
define_accumulate_weighted!( accumulate_weighted_ratio_f32, RatioF32, f32);
define_accumulate_weighted!( accumulate_weighted_thermodynamic_temperature_f32, ThermodynamicTemperatureF32, f32);
define_accumulate_weighted!( accumulate_weighted_velocity_f32, VelocityF32, f32);
define_accumulate_weighted!( accumulate_weighted_pressure_f32, PressureF32, f32);
define_accumulate_weighted!( accumulate_weighted_heat_flux_density_f32, HeatFluxDensityF32, f32);

//... and more to follow

//--- incremental averaging support

macro_rules! define_average {
    ($fname:ident, $qtype:ty) => {
        pub fn $fname (avg: &mut [$qtype], next: &[$qtype], n: usize) {
            for i in 0..avg.len() { avg[i].value = average_f64( avg[i].value, next[i].value, n) }
        }
    };
    ($fname:ident, $qtype:ty, f32) => {
        pub fn $fname (avg: &mut [$qtype], next: &[$qtype], n: usize) {
            for i in 0..avg.len() { avg[i].value = average_f32( avg[i].value, next[i].value, n) }
        }
    };
}

define_average!( average_length, Length);
define_average!( average_area, Area);
define_average!( average_angle, Angle);
define_average!( average_ratio, Ratio);
define_average!( average_thermodynamic_temperature, ThermodynamicTemperature);
define_average!( average_velocity, Velocity);
define_average!( average_pressure, Pressure);
define_average!( average_heat_flux_density, HeatFluxDensity);

define_average!( average_length_f32, LengthF32, f32);
define_average!( average_area_f32, AreaF32, f32);
define_average!( average_angle_f32, AngleF32, f32);
define_average!( average_ratio_f32, RatioF32, f32);
define_average!( average_thermodynamic_temperature_f32, ThermodynamicTemperatureF32, f32);
define_average!( average_velocity_f32, VelocityF32, f32);
define_average!( average_pressure_f32, PressureF32, f32);
define_average!( average_heat_flux_density_f32, HeatFluxDensityF32, f32);

//... and more to follow

//--- interpolation support

macro_rules! define_interpolate {
    ($fname:ident, $qtype:ty) => {
        pub fn $fname (a: &[$qtype], b: &[$qtype], r:f64)->Vec<$qtype> {
            let mut v = a.to_vec();
            for i in 0..a.len() { v[i].value = interpolate_f64( a[i].value, b[i].value, r) }
            v
        }
    };
    ($fname:ident, $qtype:ty, f32) => {
        pub fn $fname (a: &[$qtype], b: &[$qtype], r:f64)->Vec<$qtype> {
            let mut v = a.to_vec();
            for i in 0..a.len() { v[i].value = interpolate_f32( a[i].value, b[i].value, r as f32) }
            v
        }
    };
}

define_interpolate!( interpolate_length, Length);
define_interpolate!( interpolate_area, Area);
define_interpolate!( interpolate_angle, Angle);
define_interpolate!( interpolate_ratio, Ratio);
define_interpolate!( interpolate_thermodynamic_temperature, ThermodynamicTemperature);
define_interpolate!( interpolate_velocity, Velocity);
define_interpolate!( interpolate_pressure, Pressure);
define_interpolate!( interpolate_heat_flux_density, HeatFluxDensity);

define_interpolate!( interpolate_length_f32, LengthF32, f32);
define_interpolate!( interpolate_area_f32, AreaF32, f32);
define_interpolate!( interpolate_angle_f32, AngleF32, f32);
define_interpolate!( interpolate_ratio_f32, RatioF32, f32);
define_interpolate!( interpolate_thermodynamic_temperature_f32, ThermodynamicTemperatureF32, f32);
define_interpolate!( interpolate_velocity_f32, VelocityF32, f32);
define_interpolate!( interpolate_pressure_f32, PressureF32, f32);
define_interpolate!( interpolate_heat_flux_density_f32, HeatFluxDensityF32, f32);

//... and more to follow

//--- Length

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

pub fn de_length_from_millimeters <'a,D>(deserializer: D) -> Result<Length,D::Error> where D: Deserializer<'a> {
    let v: f64 = f64::deserialize(deserializer)?;
    Ok( Length::new::<millimeter>(v) )
}

pub fn de_lengths_from_millimeters <'a,D>(deserializer: D) -> Result<Vec<Length>,D::Error> where D: Deserializer<'a> {
    Ok( Vec::<f64>::deserialize(deserializer)?.into_iter().map(|v| Length::new::<millimeter>(v)).collect() )
}

pub fn de_lengths_from_millimeters_f32 <'a,D>(deserializer: D) -> Result<Vec<LengthF32>,D::Error> where D: Deserializer<'a> {
    Ok( Vec::<f32>::deserialize(deserializer)?.into_iter().map(|v| LengthF32::new::<millimeter>(v)).collect() )
}

pub fn de_lengths_from_cm_f32 <'a,D>(deserializer: D) -> Result<Vec<LengthF32>,D::Error> where D: Deserializer<'a> {
    Ok( Vec::<f32>::deserialize(deserializer)?.into_iter().map(|v| LengthF32::new::<centimeter>(v)).collect() )
}

pub fn de_lengths_from_m_f32 <'a,D>(deserializer: D) -> Result<Vec<LengthF32>,D::Error> where D: Deserializer<'a> {
    Ok( Vec::<f32>::deserialize(deserializer)?.into_iter().map(|v| LengthF32::new::<meter>(v)).collect() )
}

pub fn de_length_from_inches <'a,D>(deserializer: D) -> Result<Length,D::Error> where D: Deserializer<'a> {
    let v: f64 = f64::deserialize(deserializer)?;
    Ok( Length::new::<inch>(v) )
}

//--- Angle

pub fn de_angle_from_degree_f32 <'a,D>(deserializer: D) -> Result<AngleF32,D::Error> where D: Deserializer<'a> {
    let v: f32 = f32::deserialize(deserializer)?;
    Ok( AngleF32::new::<degree>(v) )
}

pub fn de_angles_from_degree_f32 <'a,D>(deserializer: D) -> Result<Vec<AngleF32>,D::Error> where D: Deserializer<'a> {
    Ok( Vec::<f32>::deserialize(deserializer)?.into_iter().map(|v| AngleF32::new::<degree>(v)).collect() )
}

//--- Area

pub fn ser_area_as_acres<S: Serializer> (area: &Area, s: S) -> Result<S::Ok, S::Error>  {
    let a: f64 = area.get::<acre>();
    s.serialize_f64(a)
}

pub fn ser_area_as_hectares<S: Serializer> (area: &Area, s: S) -> Result<S::Ok, S::Error>  {
    let a: f64 = area.get::<hectare>();
    s.serialize_f64(a)
}

pub fn ser_area_as_square_kilometers<S: Serializer> (area: &Area, s: S) -> Result<S::Ok, S::Error>  {
    let a: f64 = area.get::<square_kilometer>();
    s.serialize_f64(a)
}

pub fn ser_area_as_square_miles<S: Serializer> (area: &Area, s: S) -> Result<S::Ok, S::Error>  {
    let a: f64 = area.get::<square_mile>();
    s.serialize_f64(a)
}

//--- Pressure

pub fn de_pressures_from_hpa_f32 <'a,D>(deserializer: D) -> Result<Vec<PressureF32>,D::Error> where D: Deserializer<'a> {
    Ok( Vec::<f32>::deserialize(deserializer)?.into_iter().map(|v| PressureF32::new::<hectopascal>(v)).collect() )
}

//--- HeatFluxDensity

pub fn de_heatfluxdensities_from_wm2_f32 <'a,D>(deserializer: D) -> Result<Vec<HeatFluxDensityF32>,D::Error> where D: Deserializer<'a> {
    Ok( Vec::<f32>::deserialize(deserializer)?.into_iter().map(|v| HeatFluxDensityF32::new::<watt_per_square_meter>(v)).collect() )
}

//--- TermodynamicTemperature

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

pub fn de_temp_from_celsius <'a,D>(deserializer: D) -> Result<ThermodynamicTemperature,D::Error> where D: Deserializer<'a> {
    let v: f64 = f64::deserialize(deserializer)?;
    Ok( ThermodynamicTemperature::new::<degree_celsius>(v) )
}

pub fn de_temp_from_celsius_f32 <'a,D>(deserializer: D) -> Result<ThermodynamicTemperatureF32,D::Error> where D: Deserializer<'a> {
    let v: f32 = f32::deserialize(deserializer)?;
    Ok( ThermodynamicTemperatureF32::new::<degree_celsius>(v) )
}

pub fn de_temps_from_celsius_f32 <'a,D>(deserializer: D) -> Result<Vec<ThermodynamicTemperatureF32>,D::Error> where D: Deserializer<'a> {
    Ok( Vec::<f32>::deserialize(deserializer)?.into_iter().map(|v| ThermodynamicTemperatureF32::new::<degree_celsius>(v)).collect() )
}

pub fn de_temp_from_fahrenheit <'a,D>(deserializer: D) -> Result<ThermodynamicTemperature,D::Error> where D: Deserializer<'a> {
    let v: f64 = f64::deserialize(deserializer)?;
    Ok( ThermodynamicTemperature::new::<degree_fahrenheit>(v) )
}

pub fn de_temp_option_from_kelvin <'a,D>(deserializer: D) -> Result<Option<ThermodynamicTemperature>,D::Error> where D: Deserializer<'a> {
    let v: f64 = f64::deserialize(deserializer)?;
    Ok( Some(ThermodynamicTemperature::new::<kelvin>(v)) )
}

//--- Velocity

pub fn de_velocity_from_kmh <'a,D>(deserializer: D) -> Result<Velocity,D::Error> where D: Deserializer<'a> {
    let v: f64 = f64::deserialize(deserializer)?;
    Ok( Velocity::new::<kilometer_per_hour>(v) )
}

pub fn de_velocities_from_kmh <'a,D>(deserializer: D) -> Result<Vec<Velocity>,D::Error> where D: Deserializer<'a> {
    Ok( Vec::<f64>::deserialize(deserializer)?.into_iter().map(|v| Velocity::new::<kilometer_per_hour>(v)).collect() )
}

pub fn de_velocities_from_kmh_f32 <'a,D>(deserializer: D) -> Result<Vec<VelocityF32>,D::Error> where D: Deserializer<'a> {
    Ok( Vec::<f32>::deserialize(deserializer)?.into_iter().map(|v| VelocityF32::new::<kilometer_per_hour>(v)).collect() )
}

//--- Ratio

pub fn de_ratio_from_percent <'a,D>(deserializer: D) -> Result<Ratio,D::Error> where D: Deserializer<'a> {
    let v: f64 = f64::deserialize(deserializer)?;
    Ok( Ratio::new::<percent>(v) )
}

pub fn de_ratios_from_percent <'a,D>(deserializer: D) -> Result<Vec<Ratio>,D::Error> where D: Deserializer<'a> {
    Ok( Vec::<f64>::deserialize(deserializer)?.into_iter().map(|v| Ratio::new::<percent>(v)).collect() )
}

pub fn de_ratios_from_percent_f32 <'a,D>(deserializer: D) -> Result<Vec<RatioF32>,D::Error> where D: Deserializer<'a> {
    Ok( Vec::<f32>::deserialize(deserializer)?.into_iter().map(|v| RatioF32::new::<percent>(v)).collect() )
}

//--- Angle



//--- Power

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


//... and more to follow
