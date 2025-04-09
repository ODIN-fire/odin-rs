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

use std::{collections::{HashMap, VecDeque}, f64, time::Duration as StdDuration, fmt};
use odin_macro::public_struct;
use satkit::{Instant,Duration,TLE,frametransform::qteme2itrf,itrfcoord::ITRFCoord,sgp4::sgp4};
use nalgebra::{ViewStorage,base::{Matrix,ArrayStorage,dimension::{Const,Dyn}}};
use geo::{Haversine, Bearing, Destination, Point};
use uom::si::{length::meter,f64::Length};
use chrono::{DateTime,Utc};
use serde::{Deserialize,Serialize};
use odin_common::{
    angle::{normalize_360,Angle90}, 
    cartesian3::Cartesian3, 
    cartographic::{get_bbox_rad, mean_distance_rad, parallel_distance_rad, approximate_surface_centroid, Cartographic}, 
    collections::{RingDeque, SingleLookupHashMap, SortedIterable}, 
    datetime::{from_epoch_millis, ser_duration_as_fractional_secs, ser_epoch_millis, de_from_epoch_millis, de_duration_from_fractional_secs}, 
    geo::{GeoPoint, GeoPolygon, GeoRect}, 
    geo_constants::{E_EARTH, E_EARTH_SQUARED, MEAN_EARTH_RADIUS, POLAR_EARTH_RADIUS_SQUARED, EQUATORIAL_EARTH_RADIUS_SQUARED}, 
    is_same_ref, MinMaxAvg, HALF_PI, atan2, sin, cos, asin, tan, sqrt, signum,
    uom::{meters,ser_length_as_meters,de_length_from_meters}
};
use crate::{get_time_vec, SatelliteInfo, errors::{op_failed,Result,OdinOrbitalError}, tle_store::TleStore, orbitinfo::{OrbitInfo}};

type ColumnVec<'a> = Matrix<f64, Const<3>, Const<1>, ViewStorage<'a, f64, Const<3>, Const<1>, Const<1>, Const<3>>>;

/* #region configuration data **************************************************************************************/

/* #endregion coniguration data */

/// OverpassCalculator output: the segment of an orbit whose swath covers (part of) an OverpassRegion
/// this includes the regular time series trajectory in ECEF coordinates and respective start/end time of the segment
#[derive(Debug,Serialize,Deserialize)]
#[public_struct]
pub struct Overpass {
    sat_id: u32,

    max_scan_angle: Angle90, 

    #[serde(serialize_with="ser_length_as_meters", deserialize_with="de_length_from_meters")]
    mean_swath_width: Length, // note that swath width depends on altitude (i.e. varies over trajectory)

    #[serde(serialize_with="ser_length_as_meters", deserialize_with="de_length_from_meters")]
    mean_altitude: Length, // ditto

    #[serde(serialize_with="ser_epoch_millis", deserialize_with="de_from_epoch_millis")]
    start: DateTime<Utc>,

    #[serde(serialize_with="ser_epoch_millis", deserialize_with="de_from_epoch_millis")]
    end: DateTime<Utc>,

    #[serde(serialize_with="ser_duration_as_fractional_secs", deserialize_with="de_duration_from_fractional_secs")]
    time_step: StdDuration,

    trajectory: Vec<Cartesian3> // regular time series trajectory points in ECEF frame
}

impl Overpass {
    pub fn new (sat_id: u32, max_scan_angle: Angle90, ts: Duration)-> Self {
        let start = from_epoch_millis(0);
        let end = from_epoch_millis(0);
        let time_step = StdDuration::from_secs_f64( ts.as_seconds());
        let trajectory: Vec<Cartesian3> = Vec::with_capacity(1024);
        let mean_swath_width: Length = Length::new::<meter>(0.0); // computed once we have a trajectory
        let mean_altitude: Length = Length::new::<meter>(0.0); // computed once we have a trajectory

        Overpass { sat_id, max_scan_angle, mean_swath_width, mean_altitude, start, end, time_step, trajectory }
    }

    pub fn set_start (&mut self, t: Instant) {
        self.start = from_epoch_millis( (t.as_unixtime() * 1000.0) as i64);
    }

    pub fn push_trajectory_point (&mut self, p: Cartesian3) {
        self.trajectory.push( p);
    }

    pub fn set_end (&mut self, t: Instant) {
        self.end = from_epoch_millis( (t.as_unixtime() * 1000.0) as i64);
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
        self.mean_altitude = Length::new::<meter>( ((alt_first + alt_last) / 2.0).round() );
    }

    pub fn is_empty (&self)->bool {
        self.trajectory.is_empty()
    }
}

impl fmt::Display for Overpass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Overpass( sat_id:{}, start:{}, dur:{} min, step:{} s, n_points:{}, mean_alt: {:.0} m, mean_swath: {:.0} m)", 
            self.sat_id, self.start, (self.end - self.start).num_minutes(), self.time_step.as_secs(), self.trajectory.len(), 
            self.mean_altitude.get::<meter>(), self.mean_swath_width.get::<meter>())
    }
}

/// the object that performs overpass calculations
/// Note the overpass calculator makes some assumptions about the region. It has to be a concave polygon
/// and cannot exceed a hemisphere so that we get at most one overpass section in each orbit
pub struct OverpassCalculator<T: TleStore> {
    region: GeoPolygon,
    satellites: Vec<SatelliteInfo>,
    tle_store: T,
    max_tles: usize, // maximum number of TLE to keep per satellite
    ois: HashMap<u32,VecDeque<OrbitInfo>>, // sat_id -> time-sorted{OrbitInfo}
    ocs: Vec<OverpassConstraints> // one for each SatelliteInfo
}

impl <T: TleStore> OverpassCalculator<T> {
    pub fn new( region: GeoPolygon, satellites: Vec<SatelliteInfo>, tle_store: T, max_tles: usize)->Self {
        let n_regions = satellites.len();
        OverpassCalculator { region, satellites, tle_store, max_tles, ois: HashMap::new(), ocs: Vec::with_capacity( n_regions) }
    }

    /// this obtains required TLEs, computes reference orbits and OverpassRegionConstraints for each satellite
    pub async fn initialize (&mut self)->Result<()> {
        let region: Vec<Cartographic> = self.region.as_exterior_geo_points().iter().map( |p| Cartographic::from(p)).collect();

        for sat in self.satellites.iter() {
            let sat_id = sat.sat_id;
            self.tle_store.pre_fetch(sat_id).await?;
            let tles = self.tle_store.get_available_tles(sat_id);
            let ois = calculate_orbitinfos( sat_id, tles, self.max_tles);
            self.ois.insert( sat_id, ois);

            if let Some(oi) = self.get_latest_orbitinfo( sat_id) {
                let oc = OverpassConstraints::new( sat_id, sat.max_scan_angle, oi, &region);
                self.ocs.push( oc);
            } else {
                return Err( op_failed!("no OrbitInfo for {}", sat_id))
            }
        }

        Ok(())
    }

    fn get_orbitinfos_for (&self, sat_id: u32) -> Option<&VecDeque<OrbitInfo>> {
        self.ois.get( &sat_id)
    }

    fn get_latest_orbitinfo (&self, sat_id: u32) -> Option<&OrbitInfo> {
        self.ois.get( &sat_id).and_then( |ois| ois.back())
    }

    pub fn get_overpass_constraints (&self, sat_id: u32, max_scan_angle: Angle90)->Option<&OverpassConstraints> {
        self.ocs.iter().position( |oc| oc.sat_id == sat_id && oc.max_scan_angle == max_scan_angle).map( |i| &self.ocs[i])
    }

    /// get all overpasses that (partially) fall into the provided time span.
    /// This is the main reason why we have an OverpassCalculator
    /// Note this might include overpasses with start/end times that are outside the interval as we always want to check full orbits
    pub async fn get_overpasses (&mut self, sat_id: u32, max_scan_angle: Angle90, t_start: Instant, dur: Duration) -> Result<Vec<Overpass>> {

        if let Some(ois) = self.get_orbitinfos_for( sat_id) {
            if let Some(oc) = self.get_overpass_constraints(sat_id, max_scan_angle) {
                let mut oi = ois.find_closest(|o| (t_start - o.epoch()).as_seconds()).ok_or(op_failed!("no suitable ObjectInfo for {sat_id} at {t_start}"))?;
                let mut tle = oi.get_tle();
                let t_end = t_start + dur;
                let mut t = oi.get_orbit_start(t_start); 
                let mut n_steps: usize = oi.rev_sec.floor() as usize;
                let step_dur = Duration::from_seconds(1.0);
                let mut tvec: Vec<Instant> = vec![ Instant::new(0); n_steps + 20];
                let z_range = oc.z_min..oc.z_max;

                let mut overpasses: Vec<Overpass> = Vec::new();
                let mut current_overpass = Overpass::new( sat_id, max_scan_angle, step_dur);
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
                        current_overpass.set_end(tvec[i_last]);
                        if !current_overpass.is_empty() {
                            overpasses.push( current_overpass);
                            current_overpass =  Overpass::new( sat_id, max_scan_angle, step_dur);
                        }

                        is_recording = false;
                    }
                    p_last.set_undefined();

                    t = tvec[n_steps-1] + Duration::from_seconds(10.0);
                    let oi_next =  ois.find_closest(|o| (t - o.epoch()).as_seconds()).ok_or(op_failed!("no suitable OrbitInfo for {sat_id} at {t}"))?;
                    if !is_same_ref( oi, oi_next) {
                        oi = oi_next;
                        tle = oi.get_tle();
                        n_steps = oi.rev_sec.floor() as usize;
                    }                    
                    t = oi.get_orbit_start( t); // make sure we start on pole 
                }

                Ok(overpasses)

            } else { Err(op_failed!("no OverpassConstraints for {sat_id} and max_scan {max_scan_angle}")) } 
        } else { Err(op_failed!("no OrbitInfos for {sat_id}")) }
    }
}

fn calculate_orbitinfos (sat_id: u32, mut tles: Vec<TLE>, max_len: usize)->VecDeque<OrbitInfo> {
    let mut ois: VecDeque<OrbitInfo> = VecDeque::with_capacity( max_len);
    let n_tles = tles.len();

    let i0 = if n_tles > max_len { n_tles - max_len } else { 0 };
    for i in i0..n_tles { ois.push_front( OrbitInfo::new( sat_id, tles.pop().unwrap())) }

    ois
}


/* #region internal structures *******************************************************************************************/

/// struct that defines the region we want to compute overpasses for.
/// this is constructed from a concave polygon given as a list of cartographic vertices, and from satellite specific data
/// such as altitude and swath width
pub struct OverpassConstraints {
    sat_id: u32,
    max_scan_angle: Angle90,

    sin_msa: f64,  // pre-computed
    cos_msa: f64,
    
    region: Vec<Cartographic>,
    vertices: Vec<Cartesian3>,  // the ECEF vertices of the region
    normals: Vec<Cartesian3>,     // the list of unit normals for each of the planes defined by two consecutive vertices (and earth center)

    // corresponds to max and min latitude (but can be applied to both TEME and ITRF: sin(lat) * dist) as a first filter
    z_max: f64,
    z_min: f64,

    // max and min longitude (atan2(y,x) of ITRF) in radians
    lon_max: f64,
    lon_min: f64,
}

impl OverpassConstraints {
    fn new (sat_id: u32, max_scan_angle: Angle90, orbit_info: &OrbitInfo, base_region: &Vec<Cartographic>)->Self {
        // expand region so that inside-checks account for swath width
        let region = base_region.clone();

        let sin_msa = sin(max_scan_angle.radians());
        let cos_msa = cos(max_scan_angle.radians());

        let vertices: Vec<Cartesian3> = region.iter().map(|v| v.into()).collect();
        let normals: Vec<Cartesian3> = Cartesian3::normals(&vertices);
        let (z_min,z_max,lon_min,lon_max) = Self::compute_bounds( orbit_info, &vertices);

        OverpassConstraints { sat_id, max_scan_angle, sin_msa, cos_msa, region, vertices, normals,  z_max, z_min, lon_max, lon_min }
    }

    fn compute_bounds (orbit_info: &OrbitInfo, vertices: &[Cartesian3]) -> (f64,f64,f64,f64) {
        let dist = orbit_info.dist_stats.max;
        let mut z_min: f64 = f64::MAX;
        let mut z_max: f64 = f64::MIN;
        let mut lon_min: f64 = f64::MAX;
        let mut lon_max: f64 = f64::MIN;
    
        for v in vertices {
            let v = v.to_length(dist);
            if v.z < z_min { z_min = v.z }
            if v.z > z_max { z_max = v.z }
    
            let lon = atan2( v.y, v.x);
            if lon < lon_min { lon_min = lon }
            if lon > lon_max { lon_max = lon }
        }
    
        (z_min, z_max, lon_min, lon_max)
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
