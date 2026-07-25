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

use odin_macro::public_struct;
use odin_common::var_val::VariadicVal;
use crate::{OpenMeteoLocationData, OpenMeteoDataSet, utc_from_timespec, errors::{Result,op_failed}};


/// known OpenMeteoDataSets

pub type BasicEcmwfIfs = OpenMeteoLocationData<BasicEcmwfIfsData>;

#[derive(Deserialize,Debug)]
#[public_struct]
struct BasicEcmwfIfsData {
    time: Vec<String>,
    temperature_2m: Vec<f32>,
    surface_pressure: Vec<f32>,
    relative_humidity_2m: Vec<u8>,
    cloud_cover: Vec<u8>,
    wind_speed_10m: Vec<f32>,
    wind_direction_10m: Vec<u16>,
    wind_speed_80m: Vec<f32>,
    wind_direction_80m: Vec<u16>,
}

impl OpenMeteoDataSet for BasicEcmwfIfsData {
    fn dataset_name()->&'static str {
        "basic_ecmwf"
    }

    fn fields_query()->&'static str {
        "temperature_2m,surface_pressure,relative_humidity_2m,cloud_cover,wind_speed_10m,wind_direction_10m,wind_speed_80m,wind_direction_80m"
    }

    fn n_time_steps (&self)->usize {
        self.time.len()
    }

    fn nth_time_step (&self, i: usize)->Result<DateTime<Utc>> {
        if i>= self.time.len() { return Err( op_failed!("outside time_step window") ) }
        utc_from_timespec( &self.time[i])
    }

    fn time_iter (&self)->impl Iterator<Item=&String> {
        self.time.iter()
    }
}


#[derive(Deserialize,Debug)]
#[public_struct]
struct MinimalSfcData {
    time: VariadicVal<String>,
    temperature_2m: VariadicVal<f32>,
    cloud_cover: VariadicVal<u8>,
    wind_speed_10m: VariadicVal<f32>,
    wind_direction_10m: VariadicVal<u16>,
}

impl OpenMeteoDataSet for MinimalSfcData {
    fn fields_query()->&'static str {
        "temperature_2m,cloud_cover,wind_direction_10m,wind_speed_10m"
    }

    fn units_query()->&'static str {
        "temperature_2m,cloud_cover,wind_direction_10m,wind_speed_10m"
    }

    fn dataset_name ()->&'static str {
        "minimal_sfc"
    }

    fn n_time_steps (&self)->usize {
        self.time.len()
    }

    fn nth_time_step (&self, i: usize)->Result<DateTime<Utc>> {
        if i>= self.time.len() { return Err( op_failed!("outside time_step window") ) }
        utc_from_timespec( &self.time[i])
    }

    fn time_iter (&self)->impl Iterator<Item=&String> {
        self.time.iter()
    }
}

// since FW2 data sets always require an hourly range we don't need VariadicVar
#[derive(Deserialize,Debug)]
#[public_struct]
struct Fw21HourlyData {
    time: Vec<String>,
    temperature_2m: Vec<f32>,
    relative_humidity_2m: Vec<u8>,
    precipitation: Vec<f32>,
    wind_speed_10m: Vec<f32>,
    wind_direction_10m: Vec<u16>,
    wind_gusts_10m: Vec<f32>,
    shortwave_radiation: Vec<f32>,
    snow_depth: Vec<f32>,
}

impl OpenMeteoDataSet for Fw21HourlyData {
    fn fields_query ()->&'static str {
        "temperature_2m,relative_humidity_2m,precipitation,wind_speed_10m,wind_direction_10m,wind_gusts_10m,shortwave_radiation,snow_depth"
    }

    fn dataset_name ()->&'static str {
        "fw21_hourly"
    }

    fn n_time_steps (&self)->usize {
        self.time.len()
    }

    fn nth_time_step (&self, i: usize)->Result<DateTime<Utc>> {
        if i>= self.time.len() { return Err( op_failed!("outside time_step window") ) }
        utc_from_timespec( &self.time[i])
    }

    fn time_iter (&self)->impl Iterator<Item=&String> {
        self.time.iter()
    }
}


#[derive(Deserialize,Debug)]
#[public_struct]
struct DailyPrecipitation { // no need for VariadicVar - we always get vectors
    time: Vec<String>,
    precipitation_sum: Vec<f32>,
}

impl OpenMeteoDataSet for DailyPrecipitation {
    fn fields_query ()->&'static str {
        "precipitation_sum"
    }

    fn dataset_name ()->&'static str {
        "annual_daily_precip"
    }

    fn n_time_steps (&self)->usize {
        self.time.len()
    }

    fn nth_time_step (&self, i: usize)->Result<DateTime<Utc>> {
        if i>= self.time.len() { return Err( op_failed!("outside time_step window") ) }
        utc_from_timespec( &self.time[i])
    }

    fn time_iter (&self)->impl Iterator<Item=&String> {
        self.time.iter()
    }
}
