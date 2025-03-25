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

#![allow(unused)]

use std::{ffi::OsString, fs::{read_dir,File}, io::Write, path::{Path,PathBuf}, sync::LazyLock, time::Duration, collections::VecDeque};
use odin_common::{
    collections::{insert_into_ringbuffer, push_to_ringbuffer}, 
    fs::{ensure_writable_dir, filepath_contents_as_string, matching_files_in_dir, store_file_contents_in_dir}, is_same_ref
};
use serde::{Serialize,Deserialize};
use satkit::{TLE,Instant};
use std::collections::HashMap;
use chrono::{DateTime, Utc};
use reqwest::{Client,Response};
use async_trait::async_trait;
use regex::Regex;
use crate::errors::{Result,OdinOrbitalError,tle_error};

/// obtaining GP data from celestrak

/// regex to extract norad_cat_id, year, month, day and hour from TLE filename
/// e.g. tle_54234_2025-03-13_18.txt
pub static TLE_FNAME_RE: LazyLock<Regex> = LazyLock::new(|| 
    Regex::new( r"tle_(\d+)_(\d\d\d\d)-(\d\d)-(\d\d)_(\d\d)\.txt").unwrap()
);

/// regex to extract TLE lines from gp and gp_history responses - we don't need to parse the whole
/// JSON structures since we only feed the TLE lines into satkit::TLE
pub static TLE_LINES_RE: LazyLock<Regex> = LazyLock::new(||
    Regex::new( r#""TLE_LINE0": *"(.+?)",\s*"TLE_LINE1": *"(.+?)",\s*"TLE_LINE2": *"(.+?)""#).unwrap()
);

/// a trait to obtain cached TLEs from external sources 
#[async_trait]
pub trait TleStore {
    /// get the TLE for the provided NORAD-cat-id (5 digit number) and datetime and a max allowed time difference
    /// between cached and requested datetime. If none can be obtained return an error
    async fn get_tle_for_instant (&mut self, sat_id: u32, t: Instant) -> Result<TLE>;

    fn latest_epoch (&self, sat_id: u32)->Option<Instant>;

    async fn pre_fetch (&mut self, sat_id: u32)->Result<usize>;
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SpaceTrackCredentials {
    identity: String,
    password: String
}

/// SpaceTrack cookie
struct SpaceTrackCookie {
    value: String,
    created: DateTime<Utc>,
}

/// configuration data for space-track.org TLE retrieval
#[derive(Serialize,Deserialize,Debug)]
pub struct SpaceTrackConfig {
    credentials: SpaceTrackCredentials,

    max_cookie_age: Duration,  // after which we need to refresh the cookie
    max_history: usize,        // for initial history download
    max_tle_age: Duration,     // duration since last TLE after which we try to retrieve a new one

    store_files: bool,         // shall we save downloaded TLEs
    max_file_age: Duration     // how long to keep files
}


/// this is a live TleStore using space-track.org credentials to retrieve TLEs
/// note that space-track.org APIs use a login cookie with a short expiration 
pub struct SpaceTrackTleStore {
    config: SpaceTrackConfig,

    cache: HashMap<u32, VecDeque<TLE>>,  // NORAD_CAT_ID -> [epoch ordered TLE vector]
    cache_dir: Option<PathBuf>,

    cookie: Option<SpaceTrackCookie>
}

impl SpaceTrackTleStore {
    pub fn new (config: SpaceTrackConfig, cache_dir: Option<PathBuf>) -> Self {
        let cookie: Option<SpaceTrackCookie> = None;

        if let Some(dir) = &cache_dir {
            ensure_writable_dir(dir);

            match get_saved_tles( dir, config.max_file_age) {
                Ok(cache) => SpaceTrackTleStore{ config, cache, cache_dir, cookie },
                Err(_) => SpaceTrackTleStore{ config, cache: HashMap::new(), cache_dir, cookie } // TODO - should we report the error?
            }
        } else {
            SpaceTrackTleStore{ config, cache: HashMap::new(), cache_dir, cookie }
        }
    }

    async fn get_cookie_value (&mut self) -> Result<&str> {
        if let Some(cookie) = &self.cookie {
            if (Utc::now() - cookie.created).num_seconds() > self.config.max_cookie_age.as_secs() as i64 { // cookie outdated
                self.login().await?;
            }
        } else { // no cookie yet
            self.login().await?;
        }

        match &self.cookie {
            Some(cookie) => Ok( cookie.value.as_str() ),
            None => Err( tle_error!("no space-track.org cookie"))
        }
    }

    async fn login (&mut self) -> Result<()> {
        let url = "https://www.space-track.org/ajaxauth/login";
        let client = Client::new();

        let response = client
            .post( url)
            .json(&self.config.credentials)
            .send()
            .await.map_err(|e| tle_error!("space-track.org login failed: {e}"))?;

        let cookie = match response.headers().get("Set-Cookie") {
            Some(cookie) => cookie.to_str().map_err(|e| tle_error!("invalid space-track.org cookie value"))?,
            None => return Err( tle_error!("space-track.org login failed to obtain cookie")),
        };

        let cookie = SpaceTrackCookie {
            value: cookie.to_string(),
            created: Utc::now(),
        };

        self.cookie = Some(cookie);
        Ok(())
    }

    async fn get_historical_tles (&mut self, sat_id: u32, max_len: usize) -> Result<()> {
        let url = format!("https://www.space-track.org/basicspacedata/query/class/gp_history/NORAD_CAT_ID/{sat_id}/orderby/EPOCH%20desc/limit/{max_len}/");
        self.get_tles(url).await        
    }

    async fn get_current_tle (&mut self, sat_id: u32) -> Result<&TLE> {
        // both orderby and limit are not used - there only is one TLE in the response
        let url = format!("https://www.space-track.org/basicspacedata/query/class/gp/NORAD_CAT_ID/{sat_id}/orderby/EPOCH%20desc/limit/1/");
        self.get_tles(url).await?;
        
        self.cache.get(&sat_id).and_then(|tles| tles.back()).ok_or( tle_error!("unable to retrieve TLE for satellite {sat_id}"))
    }

    async fn get_tles (&mut self, url: String) -> Result<()> {
        let cookie = self.get_cookie_value().await?;
        let client = Client::new();

        let response = client
            .get(url)
            .header("Cookie", cookie.to_string())
            .send()
            .await.map_err(|e| tle_error!("space-track.org gp_history query failed: {e}"))?;

        if response.status().is_success() {
            let text =  response.text().await.map_err(|e| tle_error!("failed to obtain gp_history data {e}"))?;
            let tle_lines = parse_tle_lines(&text);

            for tl in tle_lines {
                if let Ok(tle) = TLE::load_3line( &tl.0, &tl.1, &tl.2).map_err(|e| tle_error!("3 line Satkit TLE import failed {:?}", e)) {
                    if let Some(cache_dir) = &self.cache_dir {
                        let path = cache_dir.join( tle_filename(&tle));
                        save_tle( path, &tl.0, &tl.1, &tl.2);
                    }
                    
                    add_tle( &mut self.cache, tle);
                }
            }
            Ok(())

        } else {
            Err( tle_error!("error retrieving TLE data {}", response.status()) )
        }
    }
}

#[async_trait]
impl TleStore for SpaceTrackTleStore {
    /// the main getter which first consults the cache and only queries space-track.org if no sufficiently close TLE is found
    /// this returns a clone of the cached TLE so that it can be used to flyout orbits (which requires a mutable TLE)
    async fn get_tle_for_instant (&mut self, sat_id: u32, t: Instant) -> Result<TLE> {
        let max_tle_age = self.config.max_tle_age;

        if let Some(tles) = self.cache.get( &sat_id) {
            if let Some(tle) = get_closest_tle( tles, t) {
                if is_same_ref( tle, tles.back().unwrap()) { // tle is the last one we got - check if we need a new one
                    if t > tle.epoch {
                        if ((t - tle.epoch).as_seconds() as u64) > max_tle_age.as_secs() {
                            return self.get_current_tle(sat_id).await.and_then(|tle| check_tle( t, tle, max_tle_age))
                        } else {
                            return Ok( tle.clone() ) // last tle still good
                        }
                    } 
                }
                // t within covered interval but check if close enough
                return check_tle( t, tle, max_tle_age)
            }
        } 
        self.get_current_tle(sat_id).await.and_then(|tle| check_tle( t, tle, max_tle_age))
    }

    fn latest_epoch (&self, sat_id: u32)->Option<Instant> {
        self.cache.get( &sat_id).and_then( |tles| tles.back()).map(|tle| tle.epoch)
    }

    async fn pre_fetch (&mut self, sat_id: u32)->Result<usize> {
        if let Some(epoch) = self.latest_epoch(sat_id) {
            let td = Instant::now() - epoch;
            if (td.as_seconds() as u64) < self.config.max_tle_age.as_secs() {
                return Ok( self.cache.get( &sat_id).unwrap().len() )
            }

            let max_len = td.as_days().ceil() as usize;
            self.get_historical_tles(sat_id, max_len).await?;

        } else {
            self.get_historical_tles(sat_id, self.config.max_history).await?;
        }

        self.cache.get( &sat_id).map(|tles| tles.len()).ok_or( tle_error!("no TLEs for satellite {sat_id}"))
    }
}

/* #region general helpers *******************************************************************************/

const SLOT_MINUTES: i64 = 5; // note that space-track.org sometimes has consecutive TLEs that are just a second apart

fn get_saved_tles<P: AsRef<Path>> (dir: P, max_age: Duration) -> Result<HashMap<u32,VecDeque<TLE>>> {
    let dir = dir.as_ref();
    let mut map: HashMap<u32,VecDeque<TLE>> = HashMap::new();

    if dir.is_dir() {
        for entry in read_dir(dir)? {
            if let Ok(entry) = entry {
                if let Some(fname) = entry.file_name().to_str() {
                    if let Some(cap) = TLE_FNAME_RE.captures(fname) {
                        let sat_id:u32 = cap[1].parse().unwrap();
                        let path = entry.path();
                        let text = filepath_contents_as_string(&path)?;
                        if let Ok(tle) = parse_tle_text( &text) {
                            let epoch = tle.epoch;
                            if ((Instant::now() - epoch).as_seconds() as u64) < max_age.as_secs() { // check if this TLE makes the age cut-off
                                add_tle( &mut map, tle);
                            }
                        }
                    }
                }
            }
        }
    }
    
    Ok(map)
}

fn add_tle (map: &mut HashMap<u32,VecDeque<TLE>>, tle: TLE) {
    let sat_id = tle.sat_num as u32;
    let epoch = tle.epoch;

    if let Some(tles) = map.get_mut(&sat_id) {
        for i in 0..tles.len() {
            let e = tles[i].epoch;
            if e < epoch { // parsed TLE is newer but check if we should replace prev TLE
                if ((epoch - e).as_minutes() as i64) < SLOT_MINUTES { // replace previous
                    tles[i] = tle;
                    return
                }
            } else { // stored TLE is newer than parsed one -> insert
                if ((e - epoch).as_minutes() as i64) > SLOT_MINUTES {
                    insert_into_ringbuffer( tles, i, tle);
                } // otherwise we ignore the parsed TLE
                return
            }
        }
        push_to_ringbuffer( tles, tle); // latest one

    } else { // nothing stored yet for this satellite
        let mut tles: VecDeque<TLE> = VecDeque::with_capacity(32);
        tles.push_back(tle);
        map.insert( sat_id, tles);
    }
}


fn tle_filename (tle: &TLE) -> String {
    let (year, month, day, hour, minute, second) = tle.epoch.as_datetime();
    format!("tle_{}_{:4}-{:02}-{:02}_{:02}.txt", tle.sat_num, year, month, day, hour)
}

fn parse_tle_text (text: &str)->Result<TLE> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() == 2 {
        TLE::load_2line(lines[0], lines[1]).map_err(|e| tle_error!("2 line Satkit TLE import failed {:?}", e))
    } else if lines.len() == 3 {
        TLE::load_3line(lines[0], lines[1], lines[2]).map_err(|e| tle_error!("3 line Satkit TLE import failed {:?}", e))
    } else {  
        Err( tle_error!( "response with invalid number of TLE lines {:?}", lines.len()) ) 
    }
}

pub fn parse_tle_lines (input: &str) -> Vec<(String,String,String)> {
    TLE_LINES_RE.captures_iter(input).map(|caps| {
        ( caps[1].to_string(), caps[2].to_string(), caps[3].to_string() )
    }).collect::<Vec<(String,String,String)>>()
}

fn save_tle (path: PathBuf, line0: &str, line1: &str, line2: &str) -> Result<()> {
    let mut file = File::create(path)?;
    file.write( line0.as_bytes())?;  file.write( b"\n")?;
    file.write( line1.as_bytes())?;  file.write( b"\n")?;
    file.write( line2.as_bytes())?;  file.write( b"\n")?;
    Ok(())
}

fn get_closest_tle (tles: &VecDeque<TLE>, t: Instant) -> Option<&TLE> {
    let len = tles.len();
    if len > 0 {
        // check for corner cases outside of the interval for which we have TLEs
        if tles[0].epoch >= t { return Some(&tles[0]) }
        if tles[len-1].epoch <= t { return Some(&tles[len-1]) }

        // t is within the interval - check which TLE is the closest
        let mut t_last = tles[0].epoch;
        for i in 1..len {
            let t_next = tles[i].epoch;
            if t_last < t && t_next > t {
                if (t_next - t) > (t - t_last) { return Some(&tles[i-1]) } else { return Some(&tles[i]) }
            }
            t_last = t_next;
        }
    }

    None
}

fn check_tle (t: Instant, tle: &TLE, max_age: Duration) -> Result<TLE> {
    if ((t - tle.epoch).as_seconds().abs() as u64) < max_age.as_secs() { // recent enough
        return Ok( tle.clone() )
    } else {
        return Err( tle_error!("TLE for satellite {} outside age limit: {}", tle.sat_num, t - tle.epoch))
    }
}
/* #endregion general helpers */
