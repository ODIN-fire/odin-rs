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

use std::{collections::{HashMap, VecDeque}, f64, time::Duration as StdDuration, fmt, sync::Arc, path::{Path,PathBuf}};
use odin_macro::public_struct;
use satkit::{Instant,Duration,TLE,frametransform::qteme2itrf,itrfcoord::ITRFCoord,sgp4::sgp4};
use nalgebra::{ViewStorage,base::{Matrix,ArrayStorage,dimension::{Const,Dyn}}};
use geo::{Haversine, Bearing, Destination, Point};
use uom::si::{length::meter,f64::Length};
use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::{Deserialize,Serialize};
use bit_set::BitSet;
use odin_common::{
    asin, atan2, cos, is_same_ref, signum, sin, sqrt, tan, MinMaxAvg, HALF_PI,
    angle::{normalize_360, Angle360, Angle90},
    fs::set_filepath_contents,
    cartesian3::{dist_squared, find_closest_index, Cartesian3}, 
    cartographic::{approximate_surface_centroid, earth_radius_at_geodetic_latitude, get_bbox_rad, mean_distance_rad, parallel_distance_rad, Cartographic}, 
    collections::{empty_vec, RingDeque, SingleLookupHashMap, SortedIterable},
    datetime::{de_duration_from_fractional_secs, de_from_epoch_millis, from_epoch_millis, ser_duration_as_fractional_secs, ser_epoch_millis}, 
    geo::{GeoPoint, GeoPolygon, GeoRect}, 
    geo_constants::{EQUATORIAL_EARTH_RADIUS_SQUARED, E_EARTH, E_EARTH_SQUARED, MEAN_EARTH_RADIUS, POLAR_EARTH_RADIUS_SQUARED}, 
    uom::{de_length_from_meters, meters, ser_length_as_meters},
    json_writer::{JsonWriter, JsonWritable}
};
use crate::{
    get_time_vec, instant_now, ColumnVec, OrbitalSatelliteInfo, Hotspot, 
    errors::{op_failed,Result,OdinOrbitalError}, 
    tle_store::TleStore, 
    orbitinfo::{OrbitInfo}
};

// we don't store anything that has less points as it is just skirting one of the (already margined) edges
// as a rule of thumb SSO satellites move with about 7.5km/sec
const MIN_TRAJECTORY_POINTS: usize = 10;

/* #region configuration data **************************************************************************************/

/* #endregion coniguration data */

/// OverpassCalculator output: the segment of an orbit whose swath covers (part of) an OverpassRegion
/// this includes the regular time series trajectory in ECEF coordinates and respective start/end time of the segment
/// note this structure works like a trampoline - we serialize to a pkg_cache_dir/fname file but then reset the trajectory
/// before sending it via websocket. The client then uses fname to request the full file on demand
#[derive(Debug,Serialize,Deserialize)]
#[public_struct]
pub struct Overpass {
    sat_id: u32,

    max_scan_angle: Angle90, 
    mean_swath_width: Length, // note that swath width depends on altitude (i.e. varies over trajectory)
    mean_height: Length, // ditto
    mean_orbit_duration: StdDuration,

    start: DateTime<Utc>,
    end: DateTime<Utc>,

    time_step: StdDuration,

    fname: String, // the filename to retrieve the full trajectory (from the cache dir) if trajectory is empty
    trajectory: Vec<Cartesian3> // regular time series trajectory points in ECEF frame
}

impl Overpass {
    pub fn new (sat_id: u32, max_scan_angle: Angle90, ts: Duration, mean_orb_dur: Duration)-> Self {
        let time_step = StdDuration::from_secs_f64( ts.as_seconds());
        let mean_orbit_duration = StdDuration::from_secs_f64( mean_orb_dur.as_seconds());

        // those are all set later
        let start = from_epoch_millis(0);
        let end = from_epoch_millis(0);
        let trajectory: Vec<Cartesian3> = Vec::with_capacity(1024);
        let mean_swath_width: Length = Length::new::<meter>(0.0); // computed once we have a trajectory
        let mean_altitude: Length = Length::new::<meter>(0.0); // computed once we have a trajectory
        let fname = String::with_capacity(0); 

        Overpass { sat_id, max_scan_angle, mean_swath_width, mean_height: mean_altitude, mean_orbit_duration, start, end, time_step, fname, trajectory }
    }

    pub fn set_start (&mut self, t: Instant) {
        self.start = from_epoch_millis( (t.as_unixtime() * 1000.0) as i64);
    }

    pub fn set_end (&mut self, t: Instant) {
        self.end = from_epoch_millis( (t.as_unixtime() * 1000.0) as i64);
    }

    pub fn push_trajectory_point (&mut self, p: Cartesian3) {
        self.trajectory.push( p);
    }

    fn finish (&mut self) {
        self.trajectory.shrink_to_fit();

        let p_first = &self.trajectory[0];
        let p_last = &self.trajectory[self.trajectory.len()-1];

        // we assume eccentricity is low and region extent is not exceeding a quadrant
        let swi_first = compute_swath( p_first, self.max_scan_angle.radians());
        let swi_last =  compute_swath( p_last, self.max_scan_angle.radians());
        self.mean_swath_width = Length::new::<meter>( ((swi_first.swath_width + swi_last.swath_width) / 2.0).round() );

        // same for mean altitude
        let alt_first = p_first.length() - p_first.earth_radius();
        let alt_last = p_last.length() - p_last.earth_radius();
        self.mean_height = Length::new::<meter>( ((alt_first + alt_last) / 2.0).round() );

        let start = &self.start;
        self.fname = format!("{}_{:4}-{:02}-{:02}_{:02}{:02}_{}.json", 
                           self.sat_id, start.year(), start.month(), start.day(), start.hour(), start.minute(), (self.end-start).num_minutes());
    }

    pub fn len (&self)->usize {
        self.trajectory.len()
    }

    pub fn is_empty (&self)->bool {
        self.trajectory.is_empty()
    }

    pub fn covers (&self, d: DateTime<Utc>)->bool {
        // give some leeway at the end since acquisition might have some latency - we assume download latency < orbit_dur / 2
        (d > self.start) && (d < self.end + self.mean_orbit_duration.div_f64(2.0))
    }

    // note this requires at least 2 points but anything less is just skirting one of the region corners anyways
    pub fn closest_ground_point (&self, p: &Cartesian3)->Cartesian3 {
        let traj = &self.trajectory;
        let len = traj.len();
        if len < 2 { panic!("not enough trajectory points") }  // NOTE - caller has to make sure

        let i = find_closest_index( traj, &p);
        let j = if i == 0 { 1 } else if i == len-1 { len-2 } else {
            if dist_squared( &traj[i-1], &p) > dist_squared( &traj[i+1], &p) { i+1 } else { i-1 }
        };

        if i > j {
            p.closest_point_on_plane( &traj[j], &traj[i]).to_earth_radius()
        } else {
            p.closest_point_on_plane( &traj[i], &traj[j]).to_earth_radius()
        }
    }

    pub fn bearing_to_closest_ground_point (&self, cp: Cartographic) -> Angle360 {
        let p = Cartesian3::from(cp);
        let gp = self.closest_ground_point(&p);
        let cgp = Cartographic::from(&gp);
        
        Angle360::from_degrees( cp.bearing_to( &cgp).to_degrees())
    }

    pub fn save_to (&self, dir: impl AsRef<Path>)->Result<()> {
        set_filepath_contents( dir, &self.fname, self.to_full_json().as_bytes())?;
        Ok(())
    }

    fn write_common_json_fields_to (&self, w: &mut JsonWriter) {
        w.write_field("sat_id", self.sat_id);
        w.write_field("max_scan_angle", self.max_scan_angle.degrees());
        w.write_field("mean_swath_width", self.mean_swath_width.get::<meter>().round() as i64);
        w.write_field("mean_height", self.mean_height.get::<meter>().round() as i64);
        w.write_fmt_field("mean_orbit_duration", &format!("{:.3}", self.mean_orbit_duration.as_secs_f64()));
        w.write_field("start", self.start.timestamp_millis());
        w.write_field("end", self.end.timestamp_millis());
        w.write_field("mean_orbit_duration", &format!("{:.3}", self.time_step.as_secs_f64()));
        w.write_string_field("fname", &self.fname);
    }

    pub fn to_collapsed_json (&self)->String {
        let mut w = JsonWriter::with_capacity(128);
        w.write_object(|w| self.write_common_json_fields_to(w));
        w.to_string()
    }

    pub fn to_full_json (&self)->String {
        let mut w = JsonWriter::with_capacity(128 + self.len() * 32);
        w.write_object(|w| {
            self.write_common_json_fields_to(w);
            w.write_field_with("trajectory", |w| self.trajectory.write_json_to(w));
        });
        w.to_string()
    }

    pub fn collapse (&mut self) {
        self.trajectory = empty_vec();
    }
}

pub fn save_overpasses_to (dir: impl AsRef<Path>, overpasses: &Vec<Overpass>)->Result<()> {
    for o in overpasses { 
        o.save_to( &dir)? 
    }
    Ok(())
}


impl fmt::Display for Overpass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Overpass( sat_id:{}, start:{}, dur:{} min, step:{} s, n_points:{}, mean_alt: {:.0} m, mean_swath: {:.0} m)", 
            self.sat_id, self.start, (self.end - self.start).num_minutes(), self.time_step.as_secs(), self.trajectory.len(), 
            self.mean_height.get::<meter>(), self.mean_swath_width.get::<meter>())
    }
}


/// the object that performs overpass calculations
/// Note the overpass calculator makes some assumptions about the region. It has to be a concave polygon
/// and cannot exceed a hemisphere so that we get at most one overpass section in each orbit
pub struct OverpassCalculator<T: TleStore> {
    sat_info: Arc<OrbitalSatelliteInfo>,
    tle_store: T,
    ois: VecDeque<OrbitInfo>, // time-sorted list of OrbitInfos
    oc: OverpassConstraints // calculated from region and average satellite info
}

impl <T: TleStore> OverpassCalculator<T> {
    pub fn new( sat_info: Arc<OrbitalSatelliteInfo>, region: GeoPolygon, tle_store: T)->Self {
        let oc = OverpassConstraints::new( sat_info.clone(), region);
        let ois: VecDeque<OrbitInfo> = VecDeque::with_capacity( sat_info.max_completed);
        OverpassCalculator { sat_info, tle_store, ois, oc }
    }

    /// this obtains required TLEs and computes reference orbits 
    pub async fn initialize (&mut self)->Result<()> {
        self.tle_store.pre_fetch().await?;
        self.calculate_orbitinfos();
        
        // TODO - we could re-compute the OverpassConstraints here with the height from the latest TLE but it is
        // not clear we need this precision

        Ok(())
    }

    fn calculate_orbitinfos (&mut self) {
        let max_tles = self.sat_info.max_tles;
        let mut ois: VecDeque<OrbitInfo> = VecDeque::with_capacity( max_tles);
        let mut tles = self.tle_store.get_available_tles();
        let n_tles = tles.len();
        let sat_id = self.sat_info.sat_id;

        let i0 = if n_tles > max_tles { n_tles - max_tles } else { 0 };
        for i in i0..n_tles { ois.push_front( OrbitInfo::new( sat_id, tles.pop().unwrap())) }
    
        self.ois = ois;
    }

    pub async fn get_initial_overpasses (&mut self)-> Result<Vec<Overpass>> {
        let back_dur = Duration::from_days( self.sat_info.back_days  as f64);
        let forward_dur = Duration::from_days( self.sat_info.forward_days  as f64);
        let t = instant_now() - back_dur;

        self.get_overpasses( t, back_dur + forward_dur).await
    }

    /// get all overpasses that (partially) fall into the provided time span.
    /// This is the main reason why we have an OverpassCalculator
    /// Note this might include overpasses with start/end times that are outside the provided interval as we always want to check full orbits
    pub async fn get_overpasses (&mut self, t_start: Instant, dur: Duration) -> Result<Vec<Overpass>> {
        let sat_id = self.sat_info.sat_id;
        let max_scan_angle = self.sat_info.max_scan_angle;
        let oc = &self.oc;
        let mut oi = self.ois.find_closest(|o| (t_start - o.epoch()).as_seconds())
                            .ok_or(op_failed!("no suitable OrbitInfo for {sat_id} at {t_start}"))?;
        let mut tle = oi.get_tle();
        let t_end = t_start + dur;
        let mut t = oi.get_orbit_start(t_start); 
        let mut n_steps: usize = oi.rev_sec.floor() as usize;
        let step_dur = Duration::from_seconds(1.0);
        let mut tvec: Vec<Instant> = vec![ Instant::new(0); n_steps + 20];
        let z_range = self.oc.z_min..oc.z_max;

        let mut overpasses: Vec<Overpass> = Vec::new();
        let mut current_overpass = Overpass::new( sat_id, max_scan_angle, step_dur, oi.mean_orbit_duration());
        let mut is_recording = false;
        let mut p_last = Cartesian3::undefined();

        while t < t_end {
            let mut i_last = 0;
            tvec.clear();
            for i in 0..n_steps { tvec.push(t + Duration::from_seconds( (i as f64) * 1.0)) } // initialize time vector for this rev
            let (pteme, vteme, errs) = sgp4( &mut tle, &tvec); // propagate

            for i in 0..n_steps {  
                let v = pteme.column(i);
                if z_range.contains( &v[2]) { // outer filter - needs to be efficient
                    let itrf = qteme2itrf(&tvec[i]).to_rotation_matrix() * v;
                    let p = Cartesian3::from_col( &itrf);

                    if p_last.is_undefined() { 
                        let itrf_last = qteme2itrf(&tvec[i-1]).to_rotation_matrix() * pteme.column(i-1);
                        p_last = Cartesian3::from_col( &itrf_last);
                    }

                    if oc.is_inside( &p, &p_last) {
                        if !is_recording {
                            current_overpass.set_start(tvec[i]);
                            is_recording = true;
                        }
                        current_overpass.push_trajectory_point(p.to_rounded_decimals(0)); // no point keeping decimals - sgp4 does not have enough accuracy
                        i_last = i;
                    } else {
                        if is_recording { break } // done for this revolution - note we might not get here if z-filter catches
                    }

                    p_last = p;
                } else {
                    if is_recording { break }
                }
            }

            if is_recording {
                if current_overpass.len() >= MIN_TRAJECTORY_POINTS { // we require a minimum number of points so that we can interpolate the closest ground point
                    current_overpass.set_end(tvec[i_last]);
                    current_overpass.finish();
                    overpasses.push( current_overpass);
                }
                current_overpass =  Overpass::new( sat_id, max_scan_angle, step_dur, oi.mean_orbit_duration());
                is_recording = false;
            }
            p_last.set_undefined();

            t = tvec[n_steps-1] + Duration::from_seconds(10.0);
            let oi_next =  self.ois.find_closest(|o| (t - o.epoch()).as_seconds()).ok_or(op_failed!("no suitable OrbitInfo for {sat_id} at {t}"))?;
            if !is_same_ref( oi, oi_next) {
                oi = oi_next;
                tle = oi.get_tle();
                n_steps = oi.rev_sec.floor() as usize;
            }                    
            t = oi.get_orbit_start( t); // make sure we start on pole 
        }

        Ok(overpasses)

    }
}


/* #region internal structures *******************************************************************************************/

/// struct that defines the region we want to compute overpasses for.
/// this is constructed from a concave polygon given as a list of cartographic vertices, and from satellite specific data
/// such as altitude and swath width, which we derive from an OrbitInfo.
pub struct OverpassConstraints {
    sat_info: Arc<OrbitalSatelliteInfo>,

    sin_msa: f64,  // pre-computed
    cos_msa: f64,
    
    region: Vec<Cartographic>,
    vertices: Vec<Cartesian3>,  // the ECEF vertices of the region
    normals: Vec<Cartesian3>,   // the list of unit normals for each of the planes defined by two consecutive vertices (and earth center)
    bounds: GeoRect,            // the meridian/parallel aligned hull of the region

    // corresponds to max and min latitude (but can be applied to both TEME and ITRF: sin(lat) * dist) as a first filter
    z_max: f64,
    z_min: f64,
}

impl OverpassConstraints {
    fn new (sat_info: Arc<OrbitalSatelliteInfo>, base_region: GeoPolygon)->Self {
        // TODO - check and make sure region is concave
        let region = Cartographic::vertices_of(&base_region);

        let sin_msa = sin( sat_info.max_scan_angle.radians());
        let cos_msa = cos( sat_info.max_scan_angle.radians());

        let vertices: Vec<Cartesian3> = region.iter().map(|v| v.into()).collect();
        let normals: Vec<Cartesian3> = Cartesian3::normals(&vertices);
        let bounds: GeoRect = base_region.bounds();

        let (z_min,z_max) = Self::compute_bounds( sat_info.avg_height.get::<meter>(), &vertices);

        OverpassConstraints { sat_info, sin_msa, cos_msa, region, vertices, normals,  bounds, z_max, z_min }
    }

    fn compute_bounds (avg_height: f64, vertices: &[Cartesian3]) -> (f64,f64) {
        let mut z_min: f64 = f64::MAX;
        let mut z_max: f64 = f64::MIN;
    
        for v in vertices {
            let t = v.extended_by_length( avg_height); // vertices are on the ellipsoid
            if t.z < z_min { z_min = t.z }
            if t.z > z_max { z_max = t.z }
        }
    
        (z_min, z_max)
    }

    pub fn is_inside (&self, p: &Cartesian3, p_last: &Cartesian3) -> bool {
        let norm = p_last.normal(p); // normal vec for plane defined by last 2 points

        // for low eccentricity orbits we could pre-compute this but this would at least lose out on WGS84 which could
        // matter for low altitude orbits and high scan angles
        let swi = compute_swath_internal(p, self.sin_msa, self.cos_msa); 
        let scaled_norm = norm * swi.norm_dist;

        let p1 = p + &scaled_norm; // left swath bound
        if p1.is_inside_normals( &self.normals) { return true }

        let p2 = p - &scaled_norm; // right swath bound
        if p2.is_inside_normals( &self.normals) { return true }

        false
    }
}

/* #endregion internal structures */

/* #region helper functions *****************************************************************************************/

pub struct ScanInfo {
    pub swath_width: f64, // arc distance [meter] from satellite ground point to scan horizon point
    pub sat_dist: f64,    // dist [meter] satellite to scan horizon point in meters
    pub norm_dist: f64,   // orbit plane-orthogonal distance [meter] between satellite and line through earth center and scan horizon point 
}

// internal version to save trig function calls that can be pre-computed
fn compute_swath_internal (p: &Cartesian3, sin_max_scan: f64, cos_max_scan: f64) -> ScanInfo {
    //let r = MEAN_EARTH_RADIUS; // better to compute WGS84 radius from p - radius varies in same OoM as satellite height (20km)
    let dist = p.length();

    let dist2 = dist*dist;
    let c0 = (p.z.powi(2)) / dist2;
    let c1 = ((p.x.powi(2)) + (p.y.powi(2))) / dist2;
    let c2 = c0 / POLAR_EARTH_RADIUS_SQUARED + c1 / EQUATORIAL_EARTH_RADIUS_SQUARED;
    let r = sqrt(1.0/c2);

    let c0 = sin_max_scan / r;
    let c1 = r*r - dist2;
    let c2 = dist * cos_max_scan;

    let sat_dist = c2 - sqrt(c2*c2 + c1); // arc length GP - HP
    let alpha = asin(c0 * sat_dist);  // angle EC to HP

    let swath_width = r * alpha;  // length satellite to HP
    let norm_dist = tan(alpha) * dist; // length of EC-SP normal intersection with EC-HP line

    ScanInfo { swath_width, sat_dist, norm_dist }
}

#[inline]
pub fn compute_swath (p: &Cartesian3, max_scan: f64) -> ScanInfo {
    compute_swath_internal(p, sin(max_scan), cos(max_scan))
}

pub fn geodetic_to_geocentric_latitude (lat: f64)->f64 {
    (lat.tan() * (1.0 - E_EARTH_SQUARED)).atan()     //Radians( atan(`1-e²` * tan(φ)))
}

/// inclination in radians 0..PI/2
pub fn abs_inclination (deg: f64)->f64 {
    HALF_PI - (deg.to_radians() - HALF_PI).abs()
}



/* #endregion helper functions */
