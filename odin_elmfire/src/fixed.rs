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

use odin_common::fs::{filepath_contents_as_string, filestem, matching_files_in_dir, set_filepath_contents};
use crate::{DATA_DIR, NML_RE, FixedInputStep, errors::{Result,OdinElmfireError, nml_parse_error, op_failed}, get_fixed_steps};

lazy_static! {
    // note this only accepts numeric values (which still have to be parsed in a 2nd step)
    pub static ref NML_NUM_ARR_RE: Regex = Regex::new(r#"[ \t]*([a-zA-Z0-9]+)[ \t]*=[ \t]*([0-9. \t,+-eE]+)(?:[ \t]*(?:!.*)?)"#).unwrap();
}

pub fn parse_file<P: AsRef<Path>> (path: P, n_steps: usize)->Result<Vec<FixedInputStep>> {
    let content = filepath_contents_as_string(path)?;
    parse_content( content.as_str(), n_steps)
}

pub fn parse_content (content: &str, n_steps: usize)->Result<Vec<FixedInputStep>> {
    let mut ws: Option<Vec<f64>> = None;
    let mut wd: Option<Vec<f64>> = None;
    let mut m1: Option<Vec<f64>> = None;
    let mut m10: Option<Vec<f64>> = None;
    let mut m100: Option<Vec<f64>> = None;
    let mut lh: Option<Vec<f64>> = None;
    let mut lw: Option<Vec<f64>> = None;

    for line in content.lines() {
        let res = parse_nml_array_line(line)?;
        if let Some((k, vs)) = res {
            let k = k.to_lowercase();
            match k.as_str() {
                "ws" => ws = Some(vs),
                "wd" => wd = Some(vs),
                "m1" => m1 = Some(vs),
                "m10" => m10 = Some(vs),
                "m100" => m100 = Some(vs),
                "lh" => lh = Some(vs),
                "lw" => lw = Some(vs),
                e => return Err( op_failed!("unknown variable: {}", e))
            }
        }
    }

    if let Some(ws) = ws
    && let Some(wd) = wd
    && let Some(m1) = m1
    && let Some(m10) = m10
    && let Some(m100) = m100
    && let Some(lh) = lh
    && let Some(lw) = lw {
         // note this cuts or expands all arrays to n_steps (using the last value)
        Ok( get_fixed_steps( n_steps, &ws, &wd, &m1, &m10, &m100, &lh, &lw)? )
    } else {
        Err( op_failed!("missing variables") )
    }
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
