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
use std::hash::{Hash,DefaultHasher,Hasher};
use serde::{Deserialize};
use strum::AsRefStr;

/// Known OpenMeteo query field names
/// We use a public enum with non-standard variant names to avoid explicit (error prone) mapping to &str
/// Valid names from https://open-meteo.com/en/docs (look at API URL to see query values of selected fields)
/// (the query names cannot be algorithmically derived since the height qualifiers would require explicit '_' variant name exceptions)
#[derive(Deserialize,Debug,AsRefStr,Clone,Hash,PartialEq,Eq)]
#[allow(nonstandard_style)]
pub enum FieldId {
    cloud_cover,
    dew_point_2m,
    precipitation,
    relative_humidity_2m,
    runoff,
    surface_pressure,
    temperature_2m,
    total_column_integrated_water_vapour,
    vapour_pressure_deficit,
    wind_direction_10m,
    wind_direction_100m,
    wind_gusts_10m,
    wind_speed_10m,
    wind_speed_100m,
    //... and many more to come
}

impl FieldId {
    pub fn as_list_string (fields: &[FieldId])->String {
        let fields: Vec<String> = fields.iter().map(|f| f.as_ref().to_string()).collect();
        fields.join(",")
    }
}

#[derive(Deserialize,Debug,AsRefStr,Clone,Hash,PartialEq,Eq)]
#[allow(nonstandard_style)]
pub enum ModelId {
    ecmwf_ifs
}
