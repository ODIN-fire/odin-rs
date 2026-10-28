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
#![allow(unused)]

/// support for fixed input step specifications

use std::{path::{Path,PathBuf},fs::remove_file, num::ParseFloatError, str::FromStr};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Serialize,Deserialize};

use uom::si::{
    f64::{Ratio, ThermodynamicTemperature, Velocity},
    thermodynamic_temperature::{degree_celsius,degree_fahrenheit},
    ratio::{percent,ratio},
    velocity::{mile_per_hour,meter_per_second}
};

use odin_macro::public_struct;
use odin_common::{
    fs::{filepath_contents_as_string, filestem, matching_files_in_dir, set_filepath_contents},
    angle::Angle360,
    json_writer::{JsonWritable, JsonWriter}
};
use crate::{DATA_DIR, NML_RE, errors::{Result,OdinElmfireError, nml_parse_error, op_failed}};

lazy_static! {
    // note this only accepts numeric values (which still have to be parsed in a 2nd step)
    pub static ref NML_NUM_ARR_RE: Regex = Regex::new(r#"[ \t]*([a-zA-Z0-9]+)[ \t]*=[ \t]*([0-9. \t,+-eE]+)(?:[ \t]*(?:!.*)?)"#).unwrap();
}

/// given (fixed) wx and fuel moisture input values. Each variable is a vector of hourly values (starting at the
/// base date of the Elmfire run). If a vector has less values than simulated hours its last value is used for the
/// remaining hours (see [FixedInput::value_at]).
/// Used to construct time dependent Elmfire inputs for wind and fuel moisture, and to synthesize WindNinja (HRRR
/// compatible) input when computing local wind.
/// Note that this has to be kept compatible with odin_elmfire.js `getFixed()` / `updateFixedEntries()`, which use
/// objects with value arrays in UI units: `{ws: [..mph], wd: [..deg], m1: [..%], .., tcc: [..%], t2: [..°F]}`
#[derive(Debug,Clone, Serialize, Deserialize)]
pub struct FixedInput {
    #[serde(with="vec_mph")]
    pub ws: Vec<Velocity>,  // wind speed
    pub wd: Vec<Angle360>,  // wind direction (where the wind is coming from)

    #[serde(with="vec_percent")]
    pub m1: Vec<Ratio>,     // 1h dead fuel moisture
    #[serde(with="vec_percent")]
    pub m10: Vec<Ratio>,    // 10h dead fuel moisture
    #[serde(with="vec_percent")]
    pub m100: Vec<Ratio>,   // 100h dead fuel moisture
    #[serde(with="vec_percent")]
    pub lh: Vec<Ratio>,     // live herbaceous fuel moisture
    #[serde(with="vec_percent")]
    pub lw: Vec<Ratio>,     // live woody fuel moisture

    // those are optional and only used when synthesizing WindNinja (HRRR) input
    #[serde(with="vec_percent", default)]
    pub tcc: Vec<Ratio>,    // total cloud cover
    #[serde(with="vec_fahrenheit", default)]
    pub t2: Vec<ThermodynamicTemperature>, // 2m temperature
}

impl FixedInput {
    /// create from primitive values in UI units (mph, degrees, percent, fahrenheit)
    pub fn from_values (
        ws_mph: &[f64], wd_deg: &[f64],
        m1: &[f64], m10: &[f64], m100: &[f64],
        lh: &[f64], lw: &[f64],
        tcc: &[f64], t2_f: &[f64]
    )->Result<Self> {
        let pct = |vs: &[f64]| vs.iter().map( |v| Ratio::new::<percent>(*v)).collect::<Vec<Ratio>>();

        let fixed = FixedInput {
            ws: ws_mph.iter().map( |v| Velocity::new::<mile_per_hour>(*v)).collect(),
            wd: wd_deg.iter().map( |v| Angle360::from_degrees(*v)).collect(),
            m1: pct(m1), m10: pct(m10), m100: pct(m100), lh: pct(lh), lw: pct(lw),
            tcc: pct(tcc),
            t2: t2_f.iter().map( |v| ThermodynamicTemperature::new::<degree_fahrenheit>(*v)).collect(),
        };
        fixed.check()?;
        Ok(fixed)
    }

    /// check that all mandatory variables have at least one value
    pub fn check (&self)->Result<()> {
        let missing: Vec<&str> = [
            ("ws", self.ws.is_empty()), ("wd", self.wd.is_empty()),
            ("m1", self.m1.is_empty()), ("m10", self.m10.is_empty()), ("m100", self.m100.is_empty()),
            ("lh", self.lh.is_empty()), ("lw", self.lw.is_empty())
        ].iter().filter( |(_,is_empty)| *is_empty).map( |(name,_)| *name).collect();

        if missing.is_empty() { Ok(()) } else { Err( op_failed!("missing fixed input values for: {}", missing.join(","))) }
    }

    /// the value for hour `i` - if there are less values the last one is used. Returns None for empty vectors
    pub fn value_at<T: Copy> (vs: &[T], i: usize)->Option<T> {
        if vs.is_empty() { None } else { Some( vs[i.min(vs.len()-1)]) }
    }

    /// `n` hourly values (padded with the last value if required), converted with `f`
    pub fn hourly_values<T: Copy> (vs: &[T], n: usize, f: impl Fn(T)->f64)->Vec<f64> {
        (0..n).filter_map( |i| Self::value_at(vs, i).map(&f)).collect()
    }
}

/// written in the same units as used by the UI (mph, degrees, percent, fahrenheit)
impl JsonWritable for FixedInput {
    fn write_json_to (&self, w: &mut JsonWriter) {
        let mph = |vs: &[Velocity]| vs.iter().map( |v| round1(v.get::<mile_per_hour>())).collect::<Vec<f64>>();
        let deg = |vs: &[Angle360]| vs.iter().map( |v| round1(v.degrees())).collect::<Vec<f64>>();
        let pct = |vs: &[Ratio]| vs.iter().map( |v| round1(v.get::<percent>())).collect::<Vec<f64>>();
        let fahrenheit = |vs: &[ThermodynamicTemperature]| vs.iter().map( |v| round1(v.get::<degree_fahrenheit>())).collect::<Vec<f64>>();

        w.write_object( |w| {
            w.write_slice_field( "ws", &mph(&self.ws));
            w.write_slice_field( "wd", &deg(&self.wd));
            w.write_slice_field( "m1", &pct(&self.m1));
            w.write_slice_field( "m10", &pct(&self.m10));
            w.write_slice_field( "m100", &pct(&self.m100));
            w.write_slice_field( "lh", &pct(&self.lh));
            w.write_slice_field( "lw", &pct(&self.lw));
            w.write_slice_field( "tcc", &pct(&self.tcc));
            w.write_slice_field( "t2", &fahrenheit(&self.t2));
        })
    }
}

fn round1 (v: f64)->f64 { (v * 10.0).round() / 10.0 }

/// serde support for vectors of uom quantities that are (de)serialized in non-SI units (rounded to 0.1)
macro_rules! define_vec_serde {
    ($mod_name:ident, $qty:ty, $unit:ty) => {
        mod $mod_name {
            use serde::{Deserialize, Deserializer, Serializer, ser::SerializeSeq};

            pub fn serialize<S: Serializer> (vs: &Vec<$qty>, s: S)->std::result::Result<S::Ok, S::Error> {
                let mut seq = s.serialize_seq( Some(vs.len()))?;
                for v in vs { seq.serialize_element( &super::round1( v.get::<$unit>()))?; }
                seq.end()
            }

            pub fn deserialize<'de, D: Deserializer<'de>> (d: D)->std::result::Result<Vec<$qty>, D::Error> {
                Ok( Vec::<f64>::deserialize(d)?.into_iter().map( |v| <$qty>::new::<$unit>(v)).collect() )
            }
        }
    };
}
define_vec_serde!( vec_mph, uom::si::f64::Velocity, uom::si::velocity::mile_per_hour);
define_vec_serde!( vec_percent, uom::si::f64::Ratio, uom::si::ratio::percent);
define_vec_serde!( vec_fahrenheit, uom::si::f64::ThermodynamicTemperature, uom::si::thermodynamic_temperature::degree_fahrenheit);

//--- NML support (e.g. for command line tools that read fixed input from files)

pub fn parse_file<P: AsRef<Path>> (path: P)->Result<FixedInput> {
    let content = filepath_contents_as_string(path)?;
    parse_fixed_wx_lines( content.as_str())
}

/// parse input with NML (Fortran Name List) formatted key=val,.. entries, e.g. "ws = 20,25,30"
pub fn parse_fixed_wx_lines (src: &str)->Result<FixedInput> {
    let mut ws: Vec<f64> = Vec::with_capacity(0);
    let mut wd: Vec<f64> = Vec::with_capacity(0);
    let mut m1: Vec<f64> = Vec::with_capacity(0);
    let mut m10: Vec<f64> = Vec::with_capacity(0);
    let mut m100: Vec<f64> = Vec::with_capacity(0);
    let mut lh: Vec<f64> = Vec::with_capacity(0);
    let mut lw: Vec<f64> = Vec::with_capacity(0);
    let mut tcc: Vec<f64> = Vec::with_capacity(0);
    let mut t2: Vec<f64> = Vec::with_capacity(0);

    for line in src.lines() {
        if let Some( (k,vs) ) = parse_nml_array_line(line).map_err( |e| op_failed!("invalid input line: {}", e))? {
            match k.as_str() {
                "ws" => ws = vs,
                "wd" => wd = vs,
                "m1" => m1 = vs,
                "m10" => m10 = vs,
                "m100" => m100 = vs,
                "lh" => lh = vs,
                "lw" => lw = vs,
                "tcc" => tcc = vs,
                "t2" => t2 = vs,
                o => return Err( op_failed!("unknown wx variable '{}'", o))
            }
        }
    }

    FixedInput::from_values( &ws, &wd, &m1, &m10, &m100, &lh, &lw, &tcc, &t2)
}

pub fn parse_nml_array_line (line: &str)->Result<Option<(String,Vec<f64>)>> {
    let line = line.trim();

    if line.is_empty() { return Ok(None) } // blank line
    if line.starts_with('!')  { return Ok(None) } // comment line

    if let Some(caps) = NML_NUM_ARR_RE.captures(line) {
        let key = &caps[1];
        let vs = &caps[2];

        let mut vals: Vec<f64> = Vec::new();
        for sv in vs.split(','){
            vals.push( f64::from_str(sv.trim())?);
        }
        Ok( Some((key.to_string(), vals)) )

    } else {
        Err( nml_parse_error!("invalid line: {}", line) )
    }
}
