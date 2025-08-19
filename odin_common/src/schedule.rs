/*
 * Copyright © 2024, United States Government, as represented by the Administrator of 
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

//! support module for computing schedules

use crate::datetime::Dated;
use chrono::{DateTime, TimeDelta, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc};

/// strategy of how to compact hourly schedules with outliers. Used as optional input parameter for [`get_hourly_schedule`]
pub enum Compaction {
    /// use the right edge of non-zero histogram spans (`[0,1,1,1,0,..]` is compacted into `[0,0,0,3,0,..]`) 
    RightEdge,

    /// use the right edge of spans not exceeding a given max length (in minutes) unless the edge is not
    /// just at the following minute (preventing schedules with consecutive minutes)
    BoundedRightEdge(usize)
}

/* #region hourly schedule (vector of minutes) *******************************************************************/

/// get a list of hourly minutes at which to schedule recurring events.
/// Event minutes are rounded up, i.e. 01:01 is stored as 02:00
/// Note this assumes there is some hourly periodicity in the input data set
 pub fn get_hourly_schedule<T> (dts: &[T], compact: Option<Compaction>)->Vec<u8> where T: Dated{
    let mut hist = get_hourly_schedule_histogram(dts);

    if let Some(compaction) = compact {
        match compaction {
            Compaction::RightEdge => right_edge_compact_histogram(&mut hist),
            Compaction::BoundedRightEdge(max_span) => bounded_right_edge_compact_histogram(&mut hist, max_span)
        }
    }

    schedule_from_hourly_histogram( &hist)
}

/// get `DateTime<Utc>` for the next event *after* the provided `date` based on an hourly schedule
/// given as a `& Vec<u8>` of respective hourly minutes.
/// Returned value is at full minutes (no seconds or nanoseconds)
pub fn get_next_hourly_event_dtg (date: DateTime<Utc>, sched: &Vec<u8>)->DateTime<Utc> {
    let dt = date.with_second(0).unwrap().with_nanosecond(0).unwrap(); // truncate secs/nanosecs

    let m = dt.minute() as u8;
    for i in 0..sched.len() {
        if sched[i] > m {
            if sched[i] == 60 {
                return (dt + TimeDelta::hours(1)).with_minute(0).unwrap()
            } else {
                return dt.with_minute(sched[i] as u32 % 60).unwrap()
            }
        }
    }
    
    (dt + TimeDelta::hours(1)).with_minute(sched[0] as u32 % 60).unwrap()
}


/// populate [0..60] vec with number of events falling into respective hourly minute. This is normally not
/// used directly but just called from [`get_hourly_schedule`]
pub fn get_hourly_schedule_histogram<T> (dts: &[T])->Vec<u32> where T: Dated {
    let mut hist: Vec<u32> = Vec::with_capacity(61); // 0-60
    hist.resize(61, 0);
    
    for e in dts {
        let d = e.date();
        let m: u32 = if d.second() > 0 { d.minute() + 1 } else { d.minute() }; // round up to the nearest minute
        hist[m as usize] += 1;
    }
    hist
}

/// get a vector of hourly minutes at which to schedule events. These are just the indices of non-zero histogram elements.
/// This is normally not used directly but just called from [`get_hourly_schedule`]
pub fn schedule_from_hourly_histogram (hist: &[u32])->Vec<u8> {
    let len: usize = hist.iter().fold(0, |acc,h| if *h > 0u32 { acc+1 } else { acc });
    let mut sched: Vec<u8> = Vec::with_capacity(len);

    for (m,h) in hist.iter().enumerate() {
        if *h > 0u32 { sched.push( m as u8) }
    }
    
    sched
}


//--- compaction policies

fn bounded_right_edge_compact_histogram (hist: &mut Vec<u32>, max_span: usize){
    let i_max = hist.len()-1;
    let mut i=0;
    let mut skip = hist[0] != 0;
    let mut span = 0;
    
    while i < i_max {
        if !skip { // we are accumulating non-zeros
            if hist[i+1] != 0 { // extend span
                // ..if we don't exceed max_span and we are not close to a right edge
                if (span == max_span) && (i<i_max-1) && (hist[i+2] != 0) { 
                    span = 0;
                } else {
                    hist[i+1] += hist[i];
                    hist[i] = 0;
                    span += 1;
                }
            } else { // right edge
                skip = true
            }
        } else { // we are skipping over zeros
            skip = hist[i+1] != 0;
            if !skip { 
                span = 0 
            }
        }
        i += 1;
    } 
}

fn right_edge_compact_histogram (hist: &mut Vec<u32>){
    let i_max = hist.len()-1;
    let mut i=0;
    let mut skip = hist[0] != 0;
    
    while i < i_max {
        if !skip {
            if hist[i+1] != 0 {
                hist[i+1] += hist[i];
                hist[i] = 0;
            } else { // right edge
                skip = true
            }
        } else {
            skip = hist[i+1] != 0
        }
        i += 1;
    } 
}

/* #endregion hourly schedule */