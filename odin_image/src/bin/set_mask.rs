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

use anyhow::{anyhow,Result};
use std::ops::Range;
use odin_common::define_cli;
use odin_image::Mask;

define_cli! { ARGS [about="create tile mask"] =
    spec: Option<String> [help="ranges to set (e.g. '0:7, 0-5:8, 0-24:9-24')", long,short],

    width: usize [help="number of tiles in x direction"],
    height: usize [help="number of tiles in y direction"],
    mask_file: String [help="name of mask file to store"]
}

fn main ()->Result<()> {
    let mut mask = Mask::new( ARGS.width, ARGS.height);

    if let Some(spec) = &ARGS.spec {
        let regions = parse_regions ( spec.as_str(), ARGS.width, ARGS.height)?;
        println!("{regions:?}");
        for (x_range,y_range) in regions {
            for y in y_range {
                for x in x_range.start..x_range.end {
                    mask.set( x, y);
                }
            }
        }
    }

    mask.print();

    mask.save( &ARGS.mask_file)?;
    Ok(())
}


fn parse_regions (s: &str, width: usize, height: usize)->Result<Vec<(Range<usize>,Range<usize>)>> {
    let mut ranges = Vec::new();
    let bytes = s.as_bytes();
    
    let mut x_range: Option<Range<usize>> = None;
    let mut n0: Option<usize> = None;
    let mut have_n = false;
    let mut n: usize = 0;
    let mut ni: usize = 0;

    fn finish_region(i: usize, x_range: Option<Range<usize>>, n0: Option<usize>, have_n:bool, n:usize)->Result<(Range<usize>,Range<usize>)> {
        if have_n {
            if let Some(xr) = x_range {
                let yr = if let Some(n0) = n0 {
                    Range::from(n0..n+1)
                } else {
                    Range::from(n..n+1)
                };
                Ok( (xr,yr) )
            } else {
                Err( anyhow!("region without x_range at {i}") )
            }
        } else {
            Err( anyhow!("invalid x_range at {i}") )
        }
    }

    for i in 0..bytes.len() {
        let c = bytes[i];

        if c >= b'0' && c <= b'9' {
            if have_n && i-ni != 1 {
                return Err( anyhow!("invalid number at {i}"));
            }
            n = n*10 + (c - b'0') as usize;
            have_n = true;
            ni = i;
        } else if c == b'w' {
            n = width-1;
            have_n = true;
        } else if c == b'h' {
            n = height-1;
            have_n = true;
        } else if c == b'-' {
            if have_n { 
                n0 = Some(n);
                have_n = false;
                n = 0;
            } else {
                return Err( anyhow!("invalid range spec at {i}") )
            }
        } else if c == b'*' {
            if have_n {
                return Err( anyhow!("invalid anonymous range spec at {i}") )
            } else {
                n0 = Some(0);

                n = if x_range.is_some() { height-1 } else { width-1 };
                have_n = true;
            }
        
        } else if c == b':' {
            if have_n {
                if let Some(n0) = n0 {
                    x_range = Some(Range::from(n0..n+1));
                } else {
                    x_range = Some(Range::from(n..n+1));
                }
                n0 = None;
                have_n = false;
                n = 0;
            } else {
                return Err( anyhow!("invalid x_range at {i}") )
            }

        } else if c == b',' {
            ranges.push( finish_region( i, x_range, n0, have_n, n)?);
            x_range = None;
            n0 = None;
            have_n = false;
            n = 0;
        } else if c == b' ' {
            // ignore
        }
    }
    
    ranges.push( finish_region(bytes.len(),x_range, n0, have_n, n)?);
    
    Ok(ranges)
}