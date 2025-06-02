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

//! module to compute HRRR download schedules

use chrono::{DateTime,Datelike,Timelike,Utc,SecondsFormat};
use regex::Regex;
use odin_common::{
    datetime::{elapsed_minutes_since},
    strings::{mk_string}
};

use crate::{errors::*, is_extended_forecast, HrrrConfig};

#[derive(Debug)]
pub struct HrrrSchedules {
    pub reg: Vec<u32>, // regular schedule with 0..18 forecast hours
    pub ext: Vec<u32>  // extended schedule with 0..48 forecast hours, computed at 00,06,12,18
}

impl HrrrSchedules {
    pub fn schedule_for (&self, dt: &DateTime<Utc>)->&Vec<u32> {
        if is_extended_forecast(dt) {
            &self.ext
        } else {
            &self.reg
        }
    }
}

pub async fn get_hrrr_schedules (conf: &HrrrConfig, statistic_schedules: bool)->Result<HrrrSchedules> {
    if statistic_schedules { 
        get_statistic_hrrr_schedules( &conf).await 
    } else { 
        get_estimated_hrrr_schedules( &conf)
    }
}

/// compute forecast schedules from configured estimates
pub fn get_estimated_hrrr_schedules (conf: &HrrrConfig) -> Result<HrrrSchedules> {

    fn estimate_sched (first: u32, last: u32, len: u32, delay_secs: u32)->Result<Vec<u32>> {
        if last <= first { return Err( schedule_error("invalid configured  schedule estimates")) }

        let mut sched: Vec<u32> = Vec::with_capacity(len as usize);
        let dsec = (last - first) *60 / (len-1) as u32; // schedule step vals are minutes since base hour - compute avg secs
        for i in 0..len {
            sched.push( first + ((i * dsec) + delay_secs + 30) / 60);
        }
        Ok(sched)
    }

    let delay_secs = conf.delay.as_secs() as u32;
    let reg: Vec<u32> = estimate_sched( conf.reg_first, conf.reg_last, conf.reg_len, delay_secs)?;
    let ext: Vec<u32> = estimate_sched( conf.ext_first, conf.ext_last, conf.ext_len, delay_secs)?;

    Ok( HrrrSchedules{reg,ext} )
}

/// compute forecast schedules from server directory
pub async fn get_statistic_hrrr_schedules (conf: &HrrrConfig) -> Result<HrrrSchedules> {
    let url_template = &conf.dir_url_pattern;
    let delay_minutes = (conf.delay.as_secs() + 29) as u32 / 60;

    let now = chrono::offset::Utc::now();
    let dt = if now.hour() < 12 { now - chrono::Duration::hours((now.hour() + 1).into()) } else { now }; // use prev day if not enough data points yet

    let date_spec = format!("{:04}{:02}{:02}", dt.year(), dt.month(), dt.day());
    let url = url_template.replace( "${yyyyMMdd}", date_spec.as_str());

    let response = reqwest::get(&url).await?;    
    match response.status() {
        reqwest::StatusCode::OK => {
            let txt = response.text().await?;
            parse_schedules(&txt, delay_minutes)
        }
        code => Err(schedule_error(format!("request failed with status {}", code.as_u16())))
    }
}

// get schedules for both regular (18h) and extended (48h) forecast cycles
fn parse_schedules (txt: &String, delay_minutes: u32) -> Result<HrrrSchedules> {
    // WATCH OUT - the HTML format for HRRR dir listings might change. This will cause the "unexpected directory contents.." error below
    // current line format (as of 10/21/2024): 
    // <tr><td><a href="hrrr.t00z.wrfsfcf06.grib2">hrrr.t00z.wrfsfcf06.grib2</a></td><td align="right">21-Oct-2024 00:53  </td><td align="right">137M</td></tr>
    let re = Regex::new(r#"\.grib2">hrrr\.t(\d{2})z.wrfsfcf(\d{2}).grib2[^\d]*(\d+)-(.+)-(\d{4})\s+(\d{2}):(\d{2})"#).unwrap();

    let mut avg_reg_schedule: Vec<u32> = Vec::new();
    let mut max_reg_schedule: Vec<u32> = Vec::new();
    let mut reg_data_points: Vec<u32> = Vec::new();

    let mut avg_ext_schedule: Vec<u32> = Vec::new();
    let mut max_ext_schedule: Vec<u32> = Vec::new();
    let mut ext_data_points: Vec<u32> = Vec::new();

    // get average of availability delays in minutes
    for cap in re.captures_iter(txt.as_str()) {
        if cap.len() == 8 {
            // regex makes sure those are valid numbers, cap[0] is whole match
            let bh: usize = cap[1].parse().unwrap();  // base hour (number of data points for fch) == HRRR "model cycle runtime hour"
            let fch: usize = cap[2].parse().unwrap(); // forecast hour
            let h: usize = cap[6].parse().unwrap(); // file creation minute
            let m: usize = cap[7].parse().unwrap(); // file creation minute

             // duration in minutes from begin of forecast cycle to availability of file
            let diff_minutes: u32 = if h >= bh {(h - bh)*60 + m} else {(h+24 - bh)*60 + m} as u32;

            if bh % 6 == 0 { // extended schedule at hours 0,6,12,18
                update_schedule(&mut avg_ext_schedule, &mut max_ext_schedule, &mut ext_data_points, bh, fch, h, m, diff_minutes);
            } else {
                update_schedule(&mut avg_reg_schedule, &mut max_reg_schedule, &mut reg_data_points, bh, fch, h, m, diff_minutes);
            }
        }  
    }

    if avg_reg_schedule.is_empty() || avg_ext_schedule.is_empty() { 
        Err( schedule_error("unexpected directory content - at least one schedule is empty"))

    } else {
        check_schedule( &avg_reg_schedule)?;
        check_schedule( &avg_ext_schedule)?;

        if delay_minutes > 0 {
            for i in 0..avg_reg_schedule.len() { avg_reg_schedule[i] += delay_minutes; }
            for i in 0..avg_ext_schedule.len() { avg_ext_schedule[i] += delay_minutes; }
        }      

        Ok( HrrrSchedules { reg: avg_reg_schedule, ext: avg_ext_schedule } )
    }
}


fn update_schedule (avg_schedule: &mut Vec<u32>, max_schedule: &mut Vec<u32>, data_points: &mut Vec<u32>,
                    bh: usize, fch: usize, h: usize, m: usize, diff_minutes: u32) {
    if fch >= avg_schedule.len() { // first one for this forecast hour
        avg_schedule.resize(fch+1, diff_minutes);
        max_schedule.resize(fch+1, diff_minutes);
        data_points.resize(fch+1, 1);

    } else {
        data_points[fch] += 1;
        if diff_minutes > max_schedule[fch] {
            max_schedule[fch] = diff_minutes;
        }
        if diff_minutes > avg_schedule[fch] {
            avg_schedule[fch] += (diff_minutes - avg_schedule[fch])/data_points[fch] as u32;
        } else {
            avg_schedule[fch] -= (avg_schedule[fch] - diff_minutes)/data_points[fch] as u32;
        }
    }
}


/// check if schedule is a monotonic sequence of valid delays in minutes < 60
fn check_schedule(sched: &Vec<u32>) -> Result<()> {
    if sched.is_empty() {
        Err( schedule_error("schedule is empty\n"))
    } else {
        // note that we have to check in chunks since the first extended download schedule might not be monotonic (we only get 4 of them)

        let len = sched.len();
        for i in 1..len {  // continuous forecasts for each hour (all schedules have to fit into 60min)
            if sched[i] < sched[i-1] { 
                return Err( schedule_error( format!("schedule not monotonic [{}] = {}", i, sched[i])));
            }
        }

        Ok(())
    }
}
