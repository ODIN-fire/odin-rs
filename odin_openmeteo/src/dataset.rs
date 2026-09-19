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

use serde::{Deserialize,Serialize};
use chrono::{DateTime,Utc};
use paste::paste;

use uom::si::{
    angle::degree, heat_flux_density::watt_per_square_meter, i64::Velocity, length::{centimeter, inch, meter, millimeter}, pressure::hectopascal, ratio::{percent,ratio}, thermodynamic_temperature::{degree_celsius,degree_fahrenheit}, velocity::{kilometer_per_hour,meter_per_second,mile_per_hour},
};

use odin_macro::public_struct;
use odin_common::{
    common_prefix,
    datetime::{date_from_ymd_hm, dates_from_ymd, dates_from_ymd_hm},
    num_type::{NumericType, accumulate_weighted_slices, average_slices, interpolate_slices, mut_average},
    uom::{
        ConstZero,
        AngleF32, AreaF32, HeatFluxDensityF32, LengthF32, PowerF32, PressureF32, RatioF32, ThermodynamicTemperatureF32, VelocityF32,
        de_angles_from_degree_f32, de_heatfluxdensities_from_wm2_f32, de_lengths_from_m_f32, de_lengths_from_millimeters_f32,
        de_pressures_from_hpa_f32, de_ratios_from_percent_f32, de_temps_from_celsius_f32, de_velocities_from_kmh_f32
    },
};
use crate::{LocationData, DataSet, VecDataSet, utc_from_timespec, errors::{Result,op_failed}};

//--- helper macro to define OdinOpenMeteo DataSet types

#[macro_export]
macro_rules! define_vec_dataset {
    ( $( #[$smeta:meta] )? $svis:vis $sname:ident $(: $( $dt:ty),* )? $([ $( $sopt:ident $(= $sx:expr)? ),* ])? =
        $tvis:vis time: $ttype:ty  $([ $( $topt:ident $(= $tx:expr)?),* ])? ,
        $( $( #[$fmeta:meta] )? $fvis:vis $fname:ident : Vec< $ftype:ty > $([ $( $fopt:ident $(= $fx:expr)?),* ])? ),* ) => {

        #[derive(Deserialize,Serialize,Clone,Debug $( $( , $dt)* )?)]
        $( #[serde( $( $sopt $(=$sx)? ),* )] )?
        $(#[$smeta])? $svis struct $sname {
            $( #[serde( $( $topt $(=$tx)? ),* )] )?
            $tvis time: $ttype,
            $(
                $( #[serde( $( $fopt $(=$fx)? ),* )] )?
                $(#[$fmeta])?
                $fvis $fname : Vec< $ftype >,
            )*
        }

        impl DataSet for $sname {
            fn fields ()->&'static str { odin_common::join_idents!( ",", $( $fname ),* ) }

            fn dataset_name ()->&'static str { stringify!($sname) }

            fn idw (&mut self, ds: &[(f64,&Self)]) {
                $(
                   for (w,d) in ds.iter() {
                       paste! { odin_common::uom::  [< accumulate_weighted_ $ftype:snake >] (&mut self.$fname, &d.$fname, *w) }
                   }
                )*
            }

            fn interpolate (a: &Self, b: &Self, r: f64)->Self {
                Self {
                    time: a.time.clone(),
                    $(
                        $fname :
                        paste! { odin_common::uom:: [< interpolate_ $ftype:snake >] } (&a.$fname, &b.$fname, r)
                    ),*
                }
            }


            fn average (&mut self, next: &Self, n: usize) {
                $(
                    paste! { odin_common::uom:: [< average_ $ftype:snake >] (&mut self.$fname, &next.$fname, n) }
                );*
            }

            fn filter_time_steps (&mut self, start: &DateTime<Utc>, end: &DateTime<Utc>) {
                if let Some((i0,i1)) = odin_common::collections::get_cond_range( &self.time, |t| t >= start && t <= end) {
                    odin_common::collections::shrink_vec_to_range( &mut self.time, i0, i1);
                    $( odin_common::collections::shrink_vec_to_range( &mut self.$fname, i0, i1); )*
                }
            }
        }

        impl VecDataSet for $sname {
            fn times(&self)->&[DateTime<Utc>] { &self.time }

            fn new (time: Vec<DateTime<Utc>>)->Self {
                let len = time.len();
                Self {
                    time: time.clone(),
                    $(
                       $fname : vec![ <$ftype> :: ZERO ; len ]
                    ),*
                }
            }
        }
    }
}

//--- known DataSets

define_vec_dataset!{ pub BasicData =
    pub time: Vec<DateTime<Utc>>            [deserialize_with="dates_from_ymd_hm"],
    pub temperature_2m: Vec<ThermodynamicTemperatureF32> [deserialize_with="de_temps_from_celsius_f32"],
    pub surface_pressure: Vec<PressureF32>  [deserialize_with="de_pressures_from_hpa_f32"],
    pub relative_humidity_2m: Vec<RatioF32> [deserialize_with="de_ratios_from_percent_f32"],
    pub precipitation: Vec<LengthF32>       [deserialize_with="de_lengths_from_millimeters_f32"],
    pub cloud_cover: Vec<RatioF32>          [deserialize_with="de_ratios_from_percent_f32"],
    pub wind_speed_10m: Vec<VelocityF32>    [deserialize_with="de_velocities_from_kmh_f32"],
    pub wind_direction_10m: Vec<AngleF32>   [deserialize_with="de_angles_from_degree_f32"],
    pub wind_speed_100m: Vec<VelocityF32>    [deserialize_with="de_velocities_from_kmh_f32"],
    pub wind_direction_100m: Vec<AngleF32>   [deserialize_with="de_angles_from_degree_f32"]
}

/// surface wind
define_vec_dataset! { pub WindData10m =
    pub time: Vec<DateTime<Utc>>            [deserialize_with="dates_from_ymd_hm"],
    pub wind_speed_10m: Vec<VelocityF32>    [deserialize_with="de_velocities_from_kmh_f32"],
    pub wind_direction_10m: Vec<AngleF32>   [deserialize_with="de_angles_from_degree_f32"]
}

/// minimal data set for WindNinja input
define_vec_dataset! { pub MinimalSfcData =
    pub time: Vec<DateTime<Utc>>            [deserialize_with="dates_from_ymd_hm"],
    pub temperature_2m: Vec<ThermodynamicTemperatureF32> [deserialize_with="de_temps_from_celsius_f32"],
    pub cloud_cover: Vec<RatioF32>          [deserialize_with="de_ratios_from_percent_f32"],
    pub wind_speed_10m: Vec<VelocityF32>    [deserialize_with="de_velocities_from_kmh_f32"],
    pub wind_direction_10m: Vec<AngleF32>   [deserialize_with="de_angles_from_degree_f32"]
}

/// dataset for NFDRS fuel moisture computation (also backing as WindNinja input so we add cloud_cover)
define_vec_dataset! { pub Fw21Data =
    pub time: Vec<DateTime<Utc>>            [deserialize_with="dates_from_ymd_hm"],
    pub temperature_2m: Vec<ThermodynamicTemperatureF32> [deserialize_with="de_temps_from_celsius_f32"],
    pub cloud_cover: Vec<RatioF32> [deserialize_with="de_ratios_from_percent_f32"], // not strictly required for FW21 but allows us to reuse dataset for WindNinja
    pub relative_humidity_2m: Vec<RatioF32> [deserialize_with="de_ratios_from_percent_f32"],
    pub precipitation: Vec<LengthF32> [deserialize_with="de_lengths_from_millimeters_f32"],
    pub wind_speed_10m: Vec<VelocityF32> [deserialize_with="de_velocities_from_kmh_f32"],
    pub wind_direction_10m: Vec<AngleF32> [deserialize_with="de_angles_from_degree_f32"],
    pub wind_gusts_10m: Vec<VelocityF32> [deserialize_with="de_velocities_from_kmh_f32"],
    pub shortwave_radiation: Vec<HeatFluxDensityF32> [deserialize_with="de_heatfluxdensities_from_wm2_f32"],
    pub snow_depth: Vec<LengthF32> [deserialize_with="de_lengths_from_m_f32"]
}

/// accumulated rainfall (mostly used for historical data to compute fuel moistures)
define_vec_dataset! { pub PrecipitationSum =
    pub time: Vec<DateTime<Utc>>            [deserialize_with="dates_from_ymd"],
    pub precipitation_sum: Vec<LengthF32>   [deserialize_with="de_lengths_from_millimeters_f32"]
}



/// a VecDataSet that has all the inputs we need to be able to create WindNinja input
/// TODO - should this be moved to odin_wind ?
pub trait WindNinjaDataSet: VecDataSet {
    fn wind_speed_10m (&self)->&[VelocityF32];
    fn wind_direction_10m (&self)->&[AngleF32];
    fn temperature_2m (&self)->&[ThermodynamicTemperatureF32];
    fn cloud_cover (&self)->&[RatioF32];

    // those are not required by WindNinja but for our windfield visualization
    fn wind_speed_100m (&self)->Option<&[VelocityF32]>;
    fn wind_direction_100m (&self)->Option<&[AngleF32]>;
}

impl WindNinjaDataSet for BasicData {
    // required fields
    fn wind_speed_10m (&self)->&[VelocityF32] { &self.wind_speed_10m }
    fn wind_direction_10m (&self)->&[AngleF32] { &self.wind_direction_10m }
    fn temperature_2m (&self)->&[ThermodynamicTemperatureF32] { &self.temperature_2m }
    fn cloud_cover (&self)->&[RatioF32] { &self.cloud_cover }

    fn wind_speed_100m (&self)->Option<&[VelocityF32]> { Some( &self.wind_speed_100m ) }
    fn wind_direction_100m (&self)->Option<&[AngleF32]> { Some( &self.wind_direction_100m ) }
}

impl WindNinjaDataSet for Fw21Data {
    // required fields
    fn wind_speed_10m (&self)->&[VelocityF32] { &self.wind_speed_10m }
    fn wind_direction_10m (&self)->&[AngleF32] { &self.wind_direction_10m }
    fn temperature_2m (&self)->&[ThermodynamicTemperatureF32] { &self.temperature_2m }
    fn cloud_cover (&self)->&[RatioF32] { &self.cloud_cover }

    fn wind_speed_100m (&self)->Option<&[VelocityF32]> { None }
    fn wind_direction_100m (&self)->Option<&[AngleF32]> { None }
}
