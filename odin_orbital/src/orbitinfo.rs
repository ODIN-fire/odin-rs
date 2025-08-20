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

use satkit::{sgp4::sgp4,Duration,Instant,TLE,frametransform::qteme2itrf};
use odin_common::{angle::normalize_360, cartesian3::Cartesian3, cartographic::{geodetic_latitude, Cartographic}, 
    atan, cos, pow2, sin, asin, sqrt, abs, MinMaxAvg, HALF_PI
};
use crate::{get_time_vec,ColumnVec};


/// time and ECEF longitude (degrees) of ascending or descending orbital node
/// this uses simple linear interpolation  
#[derive(Debug)]
pub struct OrbitNode { 
    pub t: f64, // unix epoch in secs
    pub longitude_deg: f64
}

/// note this is not really a pole but a z-extremum, i.e. the point farthest north or south
#[derive(Debug)]
pub struct OrbitPole { 
    pub t: f64, // unix epoch in secs
    pub latitude_deg: f64
}

/// OrbitInfo extends the mean data that is in a TLE by values that are computed from
/// propagating a reference orbit for the underlying TLE
#[derive(Debug)]
pub struct OrbitInfo {
    pub sat_id: u32, // norad_cat_id
    tle: TLE,

    incl: f64, // inclination in radians
    apparent_incl: f64, // apparent inclination in radians
    max_latitude: f64, // 

    //--- those are all computed from flying out one orbit for the given TLE
    pub dist_stats: MinMaxAvg, // min,max and avg distance

    // those are computed from the reference orbit
    pub rev_per_day: f64, 
    pub rev_sec: f64, // actual orbit seconds for this TLE ref orbit
    pub nodal_displacement: f64, // actual longitude gain for this TLE ref orbit

    pub asc_node: OrbitNode,
    pub desc_node: OrbitNode,

    pub s_pole: OrbitPole,
    pub n_pole: OrbitPole,
}

impl OrbitInfo {

    pub fn new (sat_id: u32, step_dur: Duration, tle: TLE)->Self {
        let mut dist_stats = MinMaxAvg::new();
        let mut rev_sec: f64 = 0.0;
        let mut asc_node  = OrbitNode{ t: f64::NAN, longitude_deg: 0.0};
        let mut desc_node = OrbitNode{ t: f64::NAN, longitude_deg: 0.0};
        let mut s_pole = OrbitPole{ t: f64::NAN, latitude_deg: 0.0};
        let mut n_pole = OrbitPole{ t: f64::NAN, latitude_deg: 0.0};
        
        let mean_rev_sec = (24.0 * 3600.0) / tle.mean_motion;
        let t0 = tle.epoch;
    
        let incl = tle.inclination.to_radians();
        let apparent_incl = atan( sin(incl)/(cos(incl) - 1.0/tle.mean_motion) );
        let max_latitude = geodetic_latitude(HALF_PI - abs(tle.inclination.to_radians() - HALF_PI)).to_degrees();

        let orbit_dur: Duration = Duration::from_seconds( mean_rev_sec * 1.25); // make sure we get at least one node/pole twice 
        let tvec = get_time_vec( orbit_dur, step_dur, t0);
        let n_steps = tvec.len();

        // it is about 30% faster to compute a full orbit with a 1 sec timestep as a batch than it is to go step-by-step
        let (pteme, vteme, errs) = sgp4( &mut tle.clone(), &tvec); // note this mutates the TLE, which is why we need to pass in a copy
        let p_first = Cartesian3::from_column( &pteme, 0);
        let mut p_last = Cartesian3::zero();

        for i in 0..n_steps {
            let v = pteme.column(i);
            let p = Cartesian3::new( v[0], v[1], v[2]);  // x,y,z in TEME
            let dist = p.length();  // dist same between TEME and ITRF
            let vz = vteme[(2,i)];

            dist_stats.add( dist);
    
            if i > 0 {
                // check for nodes (z changing signum)
                if p_last.z.signum() != p.z.signum() { 
                    let c = get_cartographic( &tvec[i], &v);
                    let c_last = get_cartographic(&tvec[i-1], &pteme.column(i-1));

                    let new_node = interpolate_node( tvec[i-1].as_unixtime(), &c_last, tvec[i].as_unixtime(), &c);
                    let mut node = if vz > 0.0 { &mut asc_node } else { &mut desc_node }; 
                    if !node.t.is_nan() { // we already had one - compute rev_sec from it
                        rev_sec = new_node.t - node.t;
                        break; // done - one full revolution
                    } else {
                        *node = new_node;
                    }
                }
    
                // check for poles (vz changing signum)
                let vz_last = vteme[(2,i-1)];
                if vz_last.signum() != vz.signum() {                
                    let new_pole = interpolate_pole(tvec[i-1].as_unixtime(), &p_last, vz_last, tvec[i].as_unixtime(), &p, vz);
                    let mut pole = if p.z > 0.0 { &mut n_pole } else { &mut s_pole };
                    if !pole.t.is_nan() {
                        rev_sec = new_pole.t - pole.t;
                        break; // done - one full revolution
                    } else {
                        *pole = new_pole;
                    }
                }

            } else if p.z == 0.0 { // very unlikely case first point is node
                let c = get_cartographic( &tvec[0], &v);
                let mut node = if vz > 0.0 { &mut asc_node } else { &mut desc_node }; 
                node.t = tvec[0].as_unixtime();
                node.longitude_deg = c.longitude_deg();

            } else if vz == 0.0 { // very unlikely case first point is pole
                let c = get_cartographic( &tvec[0], &v);
                let mut pole = if p.z > 0.0 { &mut n_pole } else { &mut s_pole };
                pole.t = tvec[0].as_unixtime();
                pole.latitude_deg = c.latitude_deg();
            }

            p_last = p;
        }

        let rev_per_day = (24.0 * 3600.0) / rev_sec;
        let nodal_displacement = if asc_node.t > desc_node.t { // first node was desc
            2.0 * (normalize_360( asc_node.longitude_deg) - normalize_360(desc_node.longitude_deg + 180.0))
        } else {  // first node was asc
            2.0 * (normalize_360( desc_node.longitude_deg) - normalize_360(asc_node.longitude_deg + 180.0))
        };

        OrbitInfo { sat_id, tle, incl, apparent_incl, max_latitude, dist_stats, rev_per_day, rev_sec, nodal_displacement, asc_node, desc_node, s_pole, n_pole }
    }

    //--- various metrics directly derived from TLE

    pub fn epoch (&self)->Instant {
        self.tle.epoch
    }

    pub fn inclination (&self)->f64 {
        self.tle.inclination
    }

    pub fn mean_rev_per_day (&self)->f64 {
        self.tle.mean_motion
    }

    pub fn mean_nodal_displacement (&self)->f64 {
        if self.tle.inclination > 90.0 { // retrograde orbit (moving west around globe)
            -360.0 / self.tle.mean_motion
        } else {
            360.0 / self.tle.mean_motion
        }
    }

    pub fn mean_rev_sec (&self)->f64 {
        (24.0 * 3600.0) / self.mean_rev_per_day()
    }

    pub fn mean_orbit_duration (&self)->Duration {
        Duration::from_seconds( self.mean_rev_sec() )
    }

    /// apparent incl for given latitude (all in radians)
    pub fn i_lat (&self, latitude: f64) -> f64 {
        self.apparent_incl * sqrt( 1.0 - pow2(latitude/self.max_latitude) )
    }

    /// get the orbit start (n_pole time) for the provided Instant. This is useful if we want to make sure
    /// we always start at the same orbit point (and poles are hopefully not prone to natural disasters anytime soon)
    /// note this does not apply to low-inclination orbits
    pub fn get_orbit_start (&self, t: Instant) -> Instant {
        let t = t.as_unixtime();
        let t_pole = self.n_pole.t;
        let rev_sec = self.rev_sec;

        if t > t_pole {
            Instant::from_unixtime( t_pole + ((t - t_pole) / rev_sec).floor() * rev_sec)
        } else {
            Instant::from_unixtime( t_pole - ((t_pole - t) / rev_sec).ceil() * rev_sec)
        }
    }

    /// this returns a cloned TLE that can be used for sgp4 propagation
    pub fn get_tle (&self)->TLE {
        self.tle.clone()
    }

}


/// compute orbit nodes (unix epoch secs and longitude degrees) using linear interpolation. 
/// We assume time steps are small enough to allow for linear interpolation of the trajectory over one step
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

/// compute pole (unix epoch in secs and Cartesian3 point) using linear interpolation
// we use signum of vz as the reference
// dt/dv = (t-t1)/-v1 -> t-t1 = (dt/dv * -v1) := a
fn interpolate_pole (t1: f64, p1: &Cartesian3, vz1: f64, t2: f64, p2: &Cartesian3, vz2: f64)->OrbitPole {
    let dt: f64 = t2 - t1;
    let dv: f64 = vz2 - vz1;

    let a = (dt/dv) * -vz1;
    let r = a / dt;
    let p = Cartesian3::linear_interpolation( p1, p2, r);

    let t = t1 + a;

    let c: Cartographic = p.into();
    let latitude_deg = c.latitude_deg();

    OrbitPole { t, latitude_deg }
}

pub fn get_cartographic (t: &Instant, v: &ColumnVec) -> Cartographic {
    let itrf = qteme2itrf( t).to_rotation_matrix() * v;
    let p = Cartesian3::from_col( &itrf);
    Cartographic::from(p)
}