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
use std::{hash::{Hash,DefaultHasher,Hasher}, cmp::Ordering};
use serde::{Deserialize};
use strum::AsRefStr;

/// Known HRRR query field names
/// We use a public enum with non-standard variant names to avoid explicit (error prone) mapping to &str
/// Valid names from https://nomads.ncep.noaa.gov/gribfilter.php?ds=hrrr_2d
/// (the query names cannot be algorithmically derived since the height qualifiers would require explicit '_' variant name exceptions)
#[derive(Deserialize,Debug,AsRefStr,Clone,Hash,PartialEq,Eq,Ord)]
#[allow(nonstandard_style)]
pub enum FieldId {
    PRES,
    RH,
    TCDC,
    TMP,
    UGRD,
    VGRD,
    //... and many more to follow
}

impl PartialOrd for FieldId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some( str::cmp( self.as_ref(), other.as_ref()) )
    }
}

/// Known HRRR level names
/// see FieldId for the reason why we allow nonstandard variant names
#[derive(Deserialize,Debug,AsRefStr,Clone,Hash,PartialEq,Eq,Ord)]
#[allow(nonstandard_style)]
pub enum LevelId {
    lev_surface,
    lev_2_m_above_ground,
    lev_10_m_above_ground,
    lev_80_m_above_ground,
    lev_entire_atmosphere,
    //... and many more to follow
}

impl PartialOrd for LevelId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some( str::cmp( self.as_ref(), other.as_ref()) )
    }
}
