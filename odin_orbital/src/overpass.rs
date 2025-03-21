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

use std::{f64,time::{Duration as StdDuration}};
use satkit::{Instant,Duration,TLE,frametransform::qteme2itrf,itrfcoord::ITRFCoord,sgp4::sgp4};
use nalgebra::{ViewStorage,base::{Matrix,ArrayStorage,dimension::{Const,Dyn}}};
use uom::si::{length::meter,f64::Length};
use chrono::{DateTime,Utc};
use serde::{Deserialize,Serialize};
use odin_common::{
    angle::Angle90, 
    cartesian3::Cartesian3, 
    cartographic::Cartographic, 
    geo::{GeoPoint, GeoPolygon}, 
    geo_constants::{E_EARTH, E_EARTH_SQUARED, MEAN_EARTH_RADIUS}, 
    MinMaxAvg
};
use crate::{errors::{OdinOrbitalError, Result}, tle_store::TleStore};

type ColumnVec<'a> = Matrix<f64, Const<3>, Const<1>, ViewStorage<'a, f64, Const<3>, Const<1>, Const<1>, Const<3>>>;

/* #region configuration data **************************************************************************************/

/// the configuration data for an OverPassCalculator
/// This includes the macro region and the satellites to compute overpasses for
#[derive(Debug,Serialize,Deserialize)]
pub struct OverpassCalculatorConfig {
    macro_region: Vec<GeoPoint>,           // convex polygon vertices of macro region to detect overpasses for
    satellites: Vec<OrbitalSatelliteInfo>, // the satellites/instruments to compute overpasses for

    n_past: u32,                           // number of past overpasses to compute per satellite
    past_cutoff: StdDuration,                 // how far to reach back for past overpass computation

    n_future: u32,                         // number of upcoming overpasses to compute per satellite
    future_cutoff: StdDuration                // how far to reach into the future (beware of TLE validity)
}


/// the general information about an orbital satellite 
#[derive(Debug,Serialize,Deserialize)]
pub struct OrbitalSatelliteInfo {
    sat_id: u32,
    instrument: String,
    max_scan_angle: Angle90,
}

/* #endregion coniguration data */

/// OverpassCalculator output: the segment of an orbit whose swath covers (part of) an OverpassRegion
/// this includes the regular time series trajectory in ECEF coordinates and respective start/end time of the segment
#[derive(Debug,Serialize)]
pub struct Overpass {
    sat_id: u32,
    swath_width: Length, // from ground-track to FOV boundary

    start: DateTime<Utc>,
    end: DateTime<Utc>,

    time_step: StdDuration,
    trajectory: Vec<Cartesian3> // regular time series trajectory points
}

/// the object that performs overpass calculations
pub struct OverpassCalculator<T: TleStore> {
    config: OverpassCalculatorConfig,
    tle_store: T,
    regions: Vec<OverpassRegionConstraints>
}

impl <T: TleStore> OverpassCalculator<T> {
    pub fn new( config: OverpassCalculatorConfig, tle_store: T)->Self {
        let n_regions = config.satellites.len();
        OverpassCalculator { config, tle_store, regions: Vec::with_capacity( n_regions) }
    }

    /// this obtains required TLEs, computes reference orbits with them and initializes OverpassRegions for each satellite entry
    /// from them.
    pub fn initialize (&mut self)->Result<()> {
        Ok(())
    }
}

/* #region internal structures *******************************************************************************************/

/// struct that defines the region we want to compute overpasses for.
/// this is constructed from a concave polygon given as a list of cartographic vertices, and from satellite specific data
/// such as altitude and swath width
pub struct OverpassRegionConstraints {
    sat_id: u32,
    vertices: Vec<Cartesian3>,  // the ECEF vertices of the area, computed from the macro-region

    is_retrograde: bool, // does orbits advance to west (inclination > 90 deg)
    dist: f64,    // avg distance to center of earth
    swath: f64,   // length perpendicular to ground track  (meters from ground-track to fov boundary given by max scan angle)

    normals: Vec<Cartesian3>,     // the list of unit normals for each of the planes defined by two consecutive vertices (and earth center)

    // corresponds to max and min latitude (but can be applied to both TEME and ITRF: sin(lat) * dist)
    z_max: f64,
    z_min: f64,

    // max and min longitude (atan2(y,x) of ITRF) in radians
    lon_max: f64,
    lon_min: f64,
}

impl OverpassRegionConstraints {
    pub fn new (avg_dist: Length, max_scan: Angle90, inclination: f64, vertices: Vec<Cartographic>) -> Self {
        todo!()
    }

    fn compute_normals( vertices: Vec<Cartographic>)->Vec<Cartesian3> {
        let ps: Vec<Cartesian3> = vertices.iter().map( |v| Cartesian3::from(v)).collect();
        Cartesian3::normals(&ps)
    }

    pub fn is_inside_polyhedron (&self, p: &Cartesian3)->bool {
        p.is_inside_normals( &self.normals)
    }
}

/// time and ECEF longitude [degrees] of ascending or descending orbital node
/// this uses simple linear interpolation  
#[derive(Debug)]
struct OrbitNode { 
    t: f64, // unix epoch in secs
    longitude_deg: f64
}

/// orbit information computed from reference orbits. This is used to
/// compute swath width and next overpass times so that we don't have to
/// propagate all orbits to determine overpass times for given areas
#[derive(Debug)]
struct OrbitInfo {
    t: Instant, 
    dist_stats: MinMaxAvg, // min,max and avg distance

    rev_per_day: f64, // orbital period
    rev_sec: f64, // seconds for one revolution 
    dlon_per_rev: f64, // longitude gain per rev

    asc_node: OrbitNode,
    desc_node: OrbitNode
}

impl OrbitInfo {
    fn new (tle: &TLE)->Self {
        OrbitInfo{ 
            t: tle.epoch, 
            dist_stats: MinMaxAvg::new(), 
            rev_per_day: tle.mean_motion,
            rev_sec: orbit_duration(tle).as_seconds(),
            dlon_per_rev: 360.0 / tle.mean_motion,
            asc_node: { OrbitNode{ t: f64::NAN, longitude_deg: 0.0} },
            desc_node: { OrbitNode{ t: f64::NAN, longitude_deg: 0.0} }
        }
    }

    fn set_node (&mut self, node: OrbitNode, is_ascending: bool) {
        if is_ascending {
            self.asc_node = node;
        } else {
            self.desc_node = node;
        }
    }
}

/* #endregion internal structures */

/* #region helper functions *****************************************************************************************/

/// run a full revolution for the provided TLE and compute min/max/avg dist from earth center and node data 
fn get_orbit_data (mut tle: TLE)->Result<OrbitInfo> {
    let orbit_dur = orbit_duration(&tle);
    let time_step = Duration::from_seconds(1.0);
    let t0 = tle.epoch;
    let mut oi = OrbitInfo::new(&tle);
    let tvec = get_time_vec( orbit_dur, time_step, t0);
    let n_steps = tvec.len();    

    // it is about 30% faster to compute a full orbit with a 1 sec timestep as a batch than it is to go step-by-step
    let (pteme, vteme, errs) = sgp4( &mut tle, &tvec); // note this mutates the TLE, which is why we need to pass in a copy
    let mut p_last = Cartesian3::zero();

    for i in 0..n_steps {
        let v = pteme.column(i);
        let p = Cartesian3::new( v[0], v[1], v[2]);  // x,y,z in TEME
        let dist = p.length();  // dist same between TEME and ITRF

        oi.dist_stats.add( dist);

        // check for nodes - this is the only time we need to compute ITRF/WGS84 coords for the reference orbit
        if p.z == 0.0 { // very unlikely case that node falls precisely on a time step
            let c = get_cartographic( &tvec[i], &v);
            let node = OrbitNode { t: tvec[i].as_unixtime(), longitude_deg: c.longitude_deg() };
            oi.set_node( node, vteme[(2,i)] > 0.0); // ascending if z' is positive

        } else if i > 0 && (p_last.z.signum() != p.z.signum()) {  // node is between last and this time step, interpolate
            let c = get_cartographic( &tvec[i], &v);
            let c_last = get_cartographic(&tvec[i-1], &pteme.column(i-1));
            let node = interpolate_node( tvec[i-1].as_unixtime(), &c_last, tvec[i].as_unixtime(), &c);
            oi.set_node( node, p.z > 0.0); // ascending if z is positive (->last was negative)
        }
        p_last = p;
    }

    Ok(oi)
}

fn get_cartographic (t: &Instant, v: &ColumnVec) -> Cartographic {
    let itrf = qteme2itrf( t).to_rotation_matrix() * v;
    let p = Cartesian3::from_col( &itrf);
    Cartographic::from(p)
}

/// compute orbit nodes using linear interpolation. We assume time steps are small enough to 
/// allow for linear approximation of the trajectory over one step
fn interpolate_node (t1: f64, p1: &Cartographic, t2: f64, p2: &Cartographic)->OrbitNode {
    let lon1 = p1.longitude_deg();
    let lat1 = p1.latitude_deg();
    let lon2 = p2.longitude_deg();
    let lat2 = p2.latitude_deg();

    let dlon = lon2 - lon1;
    let dlat = lat2 - lat1;
    let longitude_deg = lon1 - (lat1 * dlon/dlat);

    let dt = (t2 - t1) as f64;
    let s = (dlon.powi(2) + dlat.powi(2)).sqrt();
    let s0 = (lat1.powi(2) + (longitude_deg - lon1).powi(2)).sqrt();
    let t = t1 as f64 + dt* s0/s;

    OrbitNode { t, longitude_deg }
}

fn orbit_duration (tle: &TLE)->Duration {
    let mm = tle.mean_motion;
    let period_sec = (24.0 * 3600.0) / mm;
    Duration::from_seconds(period_sec)
}

// this has to make sure we always cover a full rev plus one time step so that we get two nodes
fn get_time_vec (orbit_duration: Duration, time_step: Duration, start_time: Instant)->Vec<Instant> {
    let n = (orbit_duration.as_seconds() / time_step.as_seconds()).ceil() as u32;
    let mut t = start_time;

    let mut tv: Vec<Instant> = Vec::with_capacity((n+1) as usize);
    for i in 0..n {
        t = start_time + Duration::from_seconds( i as f64);
        tv.push(t);
    }

    tv.push(start_time + orbit_duration);
    tv
}

/// compute the per-side swath width on the surface of a spherical earth for a given maximum scan angle
/// distance is from the center of earth (i.e. radius plus height above ground == length of ecef orbit coordinate) 
pub fn compute_swath_width (distance: Length, max_scan_angle: Angle90) -> Length {
    let r = MEAN_EARTH_RADIUS;
    let d = distance.get::<meter>();
    let msa = max_scan_angle.radians();

    let c0 = msa.sin() / r;
    let c1 = r*r - d*d;
    let c2 = d * msa.cos();

    let a = c2 - (c2*c2 + c1).sqrt();
    let alpha = (c0 * a).asin();

    Length::new::<meter>( r * alpha)
}

pub fn geodetic_to_geocentric_latitude (lat: f64)->f64 {
    (lat.tan() * (1.0 - E_EARTH_SQUARED)).atan()     //Radians( atan(`1-e²` * tan(φ)))
}

/* #endregion helper functions */
