/*
 * Copyright © 2026, United States Government, as represented by the Administrator of
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

use std::{num::NonZero, path::Path};
use kiddo::{ImmutableKdTree,dist::SquaredEuclidean};
use chrono::{DateTime,Utc};
use ndarray::prelude::*;
use odin_common::{
    collections::{remove_elements, shrink_vec_to_range}, net::{get_deserialized_cached_json_response, get_deserialized_json_response}, num_type::NumericType
};
use uom::si::{f64::Length, time::hour};
use uom::si::{length::{meter,kilometer}};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json;

use odin_common::{
    geo::{GeoPoint, GeoRect},
    num_type::{non_zero_usize},
    utm::{GeoRectMapping, OdinUtmError, UtmPoint, UtmRect, UtmZone, geo_to_utm_zone, utm_to_geo}
};
use crate::{LocationData, errors::OdinOpenMeteoError::IndexOutOfBoundsError, get_forecast_url};
use crate::{OpenMeteoConfig, VecDataSet, HourlyLocationData, errors::{Result, op_failed}, ids::ModelId};

/// represents a regular UTM grid with optional VecDataSet values for each grid point
/// Note that we assume the invariant that all set grid points share the same time step values in their VecDataSet.time, which
/// is supposed to be ordered
pub struct UtmGrid<T> {
    pub geo_center: GeoPoint,
    pub utm_center: UtmPoint,
    pub utm_rect: UtmRect,
    pub geo_rect_hull: GeoRect,
    pub cell_size: Length,
    pub n_time_steps: usize,
    pub data: Array2<Option<T>>
}

pub enum GridAlgorithm {
    NearestNeighbor,
    InverseDistanceWeighted{cutoff_factor: f64, exp: f64}
}

impl GridAlgorithm {
    pub fn idw (cutoff_factor: f64, exp: f64)->Self { Self::InverseDistanceWeighted { cutoff_factor, exp } }
}

impl<T> UtmGrid<T> where T: VecDataSet + Clone {

    pub fn from_center (geo_center: &GeoPoint, cell_size: Length, nx2: usize, ny2: usize)->Result<UtmGrid<T>> {
        let geo_center = geo_center.clone();
        let utm_center = UtmPoint::try_from( &geo_center)?.rounded();
        let utm_rect = UtmRect::from_centered_matrix(&utm_center, cell_size.get::<meter>(), nx2, ny2);

        // the geo_rect_hull for queries has to account for non-uniform mapping of coordinate systems
        let m = cell_size.get::<meter>() * 3.0;
        let geo_rect_hull = utm_rect.add_meters( -m, -m, m, m).to_geo_rect( GeoRectMapping::ExteriorGeo);

        let n_time_steps: usize = 0;
        let shape = (2*ny2+1, 2*nx2+1); // ndarray is row-major, i.e. (rows,cols)
        let data = Array2::from_elem( shape, None);

        Ok( UtmGrid { geo_center, utm_center, utm_rect, geo_rect_hull, cell_size, n_time_steps, data } )
    }

    /// NOTE - ndarray is row-major, i.e. (y,x)
    pub fn dim (&self)->(usize,usize) {
        self.data.dim()
    }

    /// the number of grid positions in horizontal (x) direction
    pub fn grid_width (&self)->usize { self.data.dim().1 }

    /// the number of grid positions in vertical (y) direction
    pub fn grid_height (&self)->usize { self.data.dim().0 }

    pub fn x_resolution (&self)->f64 {
        let (ny,nx) = self.dim();
        self.utm_rect.width() / nx as f64
    }

    pub fn y_resolution (&self)->f64 {
        let (ny,nx) = self.dim();
        self.utm_rect.height() / ny as f64
    }

    /// this uses x,y grid coordinates
    pub fn at (&self, col: usize, row: usize)->Result<&Option<T>> {
        let (h,w) = self.data.dim();

        if col < w && row < h {
            Ok( &self.data[(row,col)] )

        } else {
            Err ( IndexOutOfBoundsError( "outside grid dimensions".into()) )
        }
    }

    /// this uses x,y grid coordinates. Grid points are cell centers (utm_rect is the outer boundary of all cells)
    pub fn utm_pos_at ( &self, col: usize, row: usize)->Result<UtmPoint> {
        let (h,w) = self.data.dim();

        if col < w && row < h {
            let d = self.cell_size.get::<meter>();
            let d2 = d / 2.0;
            let utm_zone = self.utm_center.utm_zone;

            let easting = self.utm_rect.west + (col as f64 * d) + d2;
            let northing = self.utm_rect.north - (row as f64 * d) - d2;
            Ok ( UtmPoint { easting, northing, utm_zone } )

        } else {
            Err ( IndexOutOfBoundsError( "outside grid dimensions".into()) )
        }
    }

    pub fn geo_pos_at ( &self, col: usize, row: usize)->Result<GeoPoint> {
        self.utm_pos_at( col, row).map( |p| utm_to_geo(&p) )
    }

    /// the cell center points of all grid cells in row-major order
    pub fn get_utm_points (&self)->Vec<UtmPoint> {
        let (h,w) = self.data.dim();
        let d = self.cell_size.get::<meter>();
        let d2 = d / 2.0;
        let utm_zone = self.utm_center.utm_zone;
        let mut pts: Vec<UtmPoint> = Vec::with_capacity( h*w );

        for row in 0..h {
            for col in 0..w {
                let easting = self.utm_rect.west + (col as f64 * d) + d2;
                let northing = self.utm_rect.north - (row as f64 * d) - d2;
                pts.push( UtmPoint{easting,northing,utm_zone} )
            }
        }

        pts
    }

    pub fn get_geo_points (&self)->Vec<GeoPoint> {
        self.get_utm_points().into_iter().map( |p| p.into()).collect()
    }

    pub async fn get_cached_hourly_data (
        &mut self,
        config: &OpenMeteoConfig, wx_model: ModelId,
        start: &DateTime<Utc>, end: &DateTime<Utc>,
        alg: GridAlgorithm,
        path: impl AsRef<Path>
    )->Result<()>
        where T: DeserializeOwned
    {
        let url = get_forecast_url( config, &HourlyLocationData::<T>::get_bbox_query( wx_model, start, end, &self.geo_rect_hull));
        let mut data: Vec<HourlyLocationData<T>> = get_deserialized_cached_json_response( &url, path).await?;
        if !data.is_empty() {
            for d in data.iter_mut() {
                d.hourly.filter_time_steps( start, end);
            }
            self.n_time_steps = data[0].hourly.n_time_steps(); // they are all supposed to be the same
        }

        self.initialize( &data, alg)
    }

    pub async fn get_hourly_data (
        &mut self,
        config: &OpenMeteoConfig, wx_model: ModelId,
        start: &DateTime<Utc>, end: &DateTime<Utc>,
        alg: GridAlgorithm
    )->Result<()>
        where T: DeserializeOwned
    {
        let url = get_forecast_url( config, &HourlyLocationData::<T>::get_bbox_query( wx_model, start, end, &self.geo_rect_hull));

        let mut data: Vec<HourlyLocationData<T>> = get_deserialized_json_response( &url).await?;
        if !data.is_empty() {
            for d in data.iter_mut() {
                d.hourly.filter_time_steps( start, end);
            }
            self.n_time_steps = data[0].hourly.n_time_steps(); // they are all supposed to be the same
        }

        self.initialize( &data, alg)
    }

    pub fn parse_hourly_data<P: AsRef<Path>> (&mut self, alg: GridAlgorithm, path: P)->Result<()>
        where T: DeserializeOwned
    {
        let data: Vec<HourlyLocationData<T>> = serde_json::from_slice( &std::fs::read(path)?)?;
        if !data.is_empty() {
            self.n_time_steps = data[0].hourly.n_time_steps(); // they are all supposed to be the same
        }

        self.initialize( &data, alg)
    }

    fn initialize (&mut self, data: &[HourlyLocationData<T>], alg: GridAlgorithm)->Result<()>{
        match alg {
            GridAlgorithm::NearestNeighbor => self.initialize_nearest( data),
            GridAlgorithm::InverseDistanceWeighted{cutoff_factor,exp} => self.initialize_idw( data, cutoff_factor, exp)
        }
    }

    /// use the nearest neighbor method to initialize the grid. Use this for high resolution semi-gridded input
    fn initialize_nearest (&mut self, data: &[HourlyLocationData<T>])->Result<()> {
        let kd_tree = self.build_kd_tree( &data)?;

        //--- get closest data points for grid positions
        let res = self.cell_size.get::<meter>();
        let res2 = res / 2.0;
        let (ny,nx) = self.dim(); // ndarray is row-major
        let mut n_vals = Array2::from_elem( (ny,nx), 0.0);

        for j in 0..ny {
            let y = self.utm_rect.north - (j as f64 * res) - res2;
            for i in 0..nx {
                let x = self.utm_rect.west + (i as f64 * res) + res2;

                let nearest = kd_tree
                    .query( &[x,y])
                    .nearest_one::<SquaredEuclidean<f64>>()
                    .execute();

                let ld = &data[nearest.item as usize];
                self.data[(j,i)] = Some( ld.hourly.clone() ); // (row,col)

                let dist = nearest.distance.sqrt();
                let d = dist / res2;
                n_vals[(j,i)] = d;
            }
        }

        Ok(())
    }

    // use the inverse distance weighted method to initialize the grid. This is better than nearest point for coarse or very irregular input data
    fn initialize_idw (&mut self, data: &[HourlyLocationData<T>], cutoff_factor: f64, exp: f64)->Result<()> {
        let kd_tree = self.build_kd_tree( &data)?;

        let res = self.cell_size.get::<meter>();
        let res2 = res / 2.0;
        let (ny,nx) = self.dim(); // ndarray is row-major

        let cutoff = (res * cutoff_factor).powi(2); // we ignore input points that exceeed this SquaredEuclidean
        let q = 5;
        let d0 = &data[0].hourly;

        // BestQueryResultItem.distance is SquaredEculidean so we have to adjust the IDW exponent
        let exp = exp / 2.0;

        for j in 0..ny {
            let y = self.utm_rect.north - (j as f64 * res) - res2;
            for i in 0..nx {
                let x = self.utm_rect.west + (i as f64 * res) + res2;
                let cell_center = [x,y];

                let mut nearest = kd_tree
                    .query( &cell_center)
                    //.nearest_n::<SquaredEuclidean<f64>>(non_zero_usize(4)?)
                    .best_n_within::<SquaredEuclidean<f64>>(cutoff, non_zero_usize(4)?)
                    .execute();

                //remove_elements( &mut nearest, |ni| ni.distance < cutoff );
                //n_vals[(i,j)] = nearest.len();

                let mut d_acc = T::new( d0.times().to_vec());
                let mut ws: Vec<f64> = nearest.iter().map( |n| 1.0 / n.distance.powf( exp) ).collect();
                let ws_sum: f64 = ws.iter().sum();
                for w in ws.iter_mut() { *w = *w / ws_sum }
                let ds: Vec<&T> = nearest.iter().map( |n| &data[n.item as usize].hourly).collect();
                let wds: Vec<(f64,&T)> = ws.into_iter().zip( ds.into_iter()).collect();

                T::idw(&mut d_acc, &wds);
                self.data[(j,i)] = Some(d_acc); // (row,col)
            }
        }

        Ok(())
    }

    fn build_kd_tree (&self, data: &[HourlyLocationData<T>])-> Result<ImmutableKdTree<f64,2>> {
        let mut pos: Vec<[f64;2]> = Vec::with_capacity(data.len());

        for ld in data.iter() {
            let lon = ld.meta_data.longitude;
            let lat = ld.meta_data.latitude;
            let geo_p = GeoPoint::from_lon_lat_degrees(lon, lat);
            let utm_p = geo_to_utm_zone( &geo_p, self.utm_center.utm_zone).ok_or( op_failed!("invalid coordinate: {:?}", geo_p))?;
            pos.push( [utm_p.easting,utm_p.northing]);
        }

        Ok( ImmutableKdTree::new_from_slice( &pos)? )
    }

    pub fn dump_data<F> (&self, f: F) where F: Fn(&Option<T>)->String {
        let a: Array2<String> = self.data.map( |o| f(o));
        println!("{}", a);
    }

    pub fn time_step_data<U,F> (&self, step: usize, f: F)->Result<Array2<U>> where U: NumericType, F: Fn(Option<&T>,usize)->U {
        if step >= self.n_time_steps {
            Err( op_failed!("time step out of bounds") )

        } else {
            let (ny,nx) = self.dim(); // ndarray is row-major
            let mut a: Array2<U> = Array2::from_elem( self.dim(), U::zero());

            for j in 0..ny {
                for i in 0..nx {
                    let v = f( self.data[(j,i)].as_ref(), step);
                    a[(j,i)] = v;
                }
            }

            Ok( a )
        }
    }

    pub fn find_first_position<F> (&self, prop: F)->Option<(usize,usize)> where F: Fn(&Option<T>)->bool {
        let (ny,nx) = self.dim();
        for j in 0..ny {
            for i in 0..nx {
                if prop( &self.data[(j,i)]) {
                    return Some( (j,i) )
                }
            }
        }

        None
    }

    pub fn time_step_index (&self, date: &DateTime<Utc>)->Option<usize> {
        self.find_first_position( |opt| opt.is_some() )
            .and_then( |pos| self.data[pos].as_ref())
            .and_then( |d| d.time_step_index( date))
    }
}
