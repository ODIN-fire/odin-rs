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
    angle::Angle360
};
use crate::{DATA_DIR, NML_RE, errors::{Result,OdinElmfireError, nml_parse_error, op_failed}};

lazy_static! {
    // note this only accepts numeric values (which still have to be parsed in a 2nd step)
    pub static ref NML_NUM_ARR_RE: Regex = Regex::new(r#"[ \t]*([a-zA-Z0-9]+)[ \t]*=[ \t]*([0-9. \t,+-eE]+)(?:[ \t]*(?:!.*)?)"#).unwrap();
}

/// structure with given wx and fuel data values for the same time point.
/// Used to construct time/location dependent Elmfire inputs for wind and fuel moisture
/// Also used to synthesize WindNinja (HRRR compatible) input when computing local wind
#[derive(Debug,Clone, Serialize, Deserialize)]
#[public_struct]
struct FixedInputStep {
    ws: Velocity,  // wind speed
    wd: Angle360,  // wind direction

    m1: Ratio,     // 1h dead fuel moisture
    m10: Ratio,    // 10h dead fuel moisture
    m100: Ratio,   // 100h dead fuel moisture
    lh: Ratio,     // live herbaceous fuel moisture
    lw: Ratio,     // live woody fuel moisture

    // those are only used when synthesizing WindNinja (HRRR) input
    tcc: Ratio,     // total cloud cover
    t2: ThermodynamicTemperature, // 2m temperature
}

pub fn parse_file<P: AsRef<Path>> (path: P, n_steps: usize)->Result<Vec<FixedInputStep>> {
    let content = filepath_contents_as_string(path)?;
    parse_fixed_wx_lines( content.as_str(), n_steps)
}

/// parse input with NML (Fortran Name List) formatted key=val,.. entries
pub fn parse_fixed_wx_lines (src: &str, n_steps: usize)->Result<Vec<FixedInputStep>> {
    let mut ws: Vec<f64> = Vec::with_capacity(0);
    let mut wd: Vec<f64> = Vec::with_capacity(0);
    let mut m1: Vec<f64> = Vec::with_capacity(0);
    let mut m10: Vec<f64> = Vec::with_capacity(0);
    let mut m100: Vec<f64> = Vec::with_capacity(0);
    let mut lh: Vec<f64> = Vec::with_capacity(0);
    let mut lw: Vec<f64> = Vec::with_capacity(0);
    let mut tcc: Vec<f64> = Vec::with_capacity(0);
    let mut t2: Vec<f64> = Vec::with_capacity(0);

    println!("@@ fixed src: '{}'", src);

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

    get_fixed_steps( n_steps, &ws, &wd, &m1, &m10, &m100, &lh, &lw, &tcc, &t2)
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

pub fn fixed_src_from_steps (steps: &[FixedInputStep])->String {
    let ws = steps.iter().map( |s| format!("{:.1}",s.ws.get::<mile_per_hour>())).collect::<Vec<String>>().join(",");
    let wd = steps.iter().map( |s| format!("{:.0}", s.wd.degrees())).collect::<Vec<String>>().join(",");
    let m1 = steps.iter().map( |s| format!("{:.0}", s.m1.get::<percent>())).collect::<Vec<String>>().join(",");
    let m10 = steps.iter().map( |s| format!("{:.0}", s.m10.get::<percent>())).collect::<Vec<String>>().join(",");
    let m100 = steps.iter().map( |s| format!("{:.0}", s.m100.get::<percent>())).collect::<Vec<String>>().join(",");
    let lh = steps.iter().map( |s| format!("{:.0}", s.lh.get::<percent>())).collect::<Vec<String>>().join(",");
    let lw = steps.iter().map( |s| format!("{:.0}", s.lw.get::<percent>())).collect::<Vec<String>>().join(",");

    format!("ws={ws}\nwd={wd}\nm1={m1}\nm10={m10}\nm100={m100}\nlh={lh}\nlw={lw}\n")
}

/// turn lists of primitive input values into a vec of FxiedInputSteps. All provided input value slices have to be non-empty.
/// If an input value slice has not enough elements for a time step it is filled up with the last element
pub fn get_fixed_steps (
    n_steps: usize,
    ws_mph: &[f64], wd_deg: &[f64],
    m1: &[f64], m10: &[f64], m100: &[f64],
    lh: &[f64], lw: &[f64],
    tcc: &[f64], t2: &[f64]
)->Result<Vec<FixedInputStep>>
{
    if ws_mph.is_empty() || wd_deg.is_empty() || m1.is_empty() || m10.is_empty() || m100.is_empty() || lh.is_empty() || lw.is_empty(){
        return Err( op_failed!("missing FixedInputStep component"))
    }

    let mut vs: Vec<FixedInputStep> = Vec::with_capacity( n_steps);

    let ws_len = ws_mph.len();
    let wd_len = wd_deg.len();
    let m1_len = m1.len();
    let m10_len = m10.len();
    let m100_len = m100.len();
    let lh_len = lh.len();
    let lw_len = lw.len();
    let tcc_len = tcc.len();
    let t2_len = t2.len();

    for i in 0..n_steps {
        let ws = Velocity::new::<mile_per_hour>( if i < ws_len { ws_mph[i] } else { ws_mph[ws_len-1] });
        let wd = Angle360::from_degrees( if i < wd_len { wd_deg[i] } else { wd_deg[wd_len-1] });
        let m1 = Ratio::new::<percent>( if i < m1_len { m1[i] } else { m1[m1_len-1] });
        let m10 = Ratio::new::<percent>( if i < m10_len { m10[i] } else { m10[m10_len-1] });
        let m100 = Ratio::new::<percent>( if i < m100_len { m100[i] } else { m100[m100_len-1] });
        let lh = Ratio::new::<percent>( if i < lh_len { lh[i] } else { lh[lh_len-1] });
        let lw = Ratio::new::<percent>( if i < lw_len { lw[i] } else { lw[lw_len-1] });

        let tcc = Ratio::new::<percent>( if !tcc.is_empty() {if i < tcc_len { tcc[i]} else {tcc[tcc_len-1]}} else {0.0} );
        let t2 = ThermodynamicTemperature::new::<degree_fahrenheit>( if !t2.is_empty() {if i < t2_len { t2[i]} else {t2[t2_len-1]}} else {0.0} );

        vs.push( FixedInputStep{ws,wd, m1,m10,m100,lh,lw, tcc,t2});
    }

    Ok( vs )
}
