/*
 * Copyright (c) 2024, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The ODIN - Open Data Integration Framework is licensed under the
 * Apache License, Version 2.0 (the "License"); you may not use this file
 * except in compliance with the License. You may obtain a copy of the
 * License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
use std::f64::consts::PI;
use std::collections::HashMap;
use gdal::{Dataset, Metadata, MetadataEntry};
use meshgridrs::{meshgrid, Indexing};
use ndarray::{Array, ArrayBase, OwnedRepr, Dim, IxDynImpl};
use odin_common::geo::LatLon;
use crate::{errors::*, GoesRData};

 #[derive(Debug)]
pub struct Projection { // goesr projection needed to convert to latlon grid
    pub grid_mapping_name: String,
    pub inverse_flattening: f64,
    pub latitude_of_projection_origin: f64,
    pub longitude_of_projection_origin: f64,
    pub long_name: String,
    pub perspective_point_height: f64,
    pub semi_major_axis: f64,
    pub semi_minor_axis: f64,
    pub sweep_angle_axis: char,
}

pub fn get_projection(ds: &Dataset) -> Result<Projection> {
    // take dataset
    // iterate over metadata
    let mut projection_map = HashMap::new();
   
    for MetadataEntry { domain:_, key, value } in ds.metadata() {
        let key_mut = key.clone();
        let cleaned_key = key_mut.strip_prefix("goes_imager_projection#").unwrap_or_default().to_owned();
        projection_map.insert(cleaned_key, value);
    }
    // store metadata into projection struct
    let projection = Projection {
        grid_mapping_name: projection_map["grid_mapping_name"].to_string(),
        inverse_flattening: projection_map["inverse_flattening"].parse()?,
        latitude_of_projection_origin: projection_map["latitude_of_projection_origin"].parse()?,
        longitude_of_projection_origin: projection_map["longitude_of_projection_origin"].parse()?,
        long_name: projection_map["long_name"].to_string(),
        perspective_point_height: projection_map["perspective_point_height"].parse()?,
        semi_major_axis: projection_map["semi_major_axis"].parse()?,
        semi_minor_axis: projection_map["semi_minor_axis"].parse()?,
        sweep_angle_axis: projection_map["sweep_angle_axis"].chars().next().unwrap(),
    };
    Ok(projection)
}

pub struct XYGridParams {
    x_start: f64,
    x_scale: f64,
    y_start: f64,
    y_scale: f64
}

pub fn get_xy_grid_params (ds: &Dataset) -> Result<XYGridParams> {
    // iterate over metadata
    let mut grid_map = HashMap::new();

    for MetadataEntry { domain:_, key, value } in ds.metadata() {
        grid_map.insert(key, value);
    }
    // store metadata into xy grid params struct
    let xy_grid_params = XYGridParams {
        x_start: grid_map["x#add_offset"].parse()?,
        x_scale: grid_map["x#scale_factor"].parse()?,
        y_start: grid_map["y#add_offset"].parse()?,
        y_scale: grid_map["y#scale_factor"].parse()?,
    };
    Ok(xy_grid_params)
}

pub struct LatLonGrid {
    lat_grid: ArrayBase<OwnedRepr<f64>, Dim<IxDynImpl>>,
    lon_grid: ArrayBase<OwnedRepr<f64>, Dim<IxDynImpl>>
}

#[derive(Debug,Clone, Copy)]
pub struct GoesRBoundingBox {
    pub ne: LatLon,
    pub nw:LatLon,
    pub sw: LatLon,
    pub se: LatLon
}

pub fn get_bounds(x: usize, y: usize, lat_lon_grid: &LatLonGrid) -> Result<GoesRBoundingBox > {
    let dx = if x > 0 {
        ((lat_lon_grid.lat_grid[[x,y]] - lat_lon_grid.lat_grid[[x-1,y]])/2.0, (lat_lon_grid.lon_grid[[x,y]] - lat_lon_grid.lon_grid[[x-1,y]])/2.0) // delta to neighboring x
    } else {
        ((lat_lon_grid.lat_grid[[x+1,y]] - lat_lon_grid.lat_grid[[x,y]])/2.0,(lat_lon_grid.lon_grid[[x+1,y]] - lat_lon_grid.lon_grid[[x,y]])/2.0) // delta to neighboring x
    };

    let dy =if y > 0 {
        ((lat_lon_grid.lat_grid[[x,y]] - lat_lon_grid.lat_grid[[x,y-1]])/2.0, (lat_lon_grid.lat_grid[[x,y]] - lat_lon_grid.lat_grid[[x,y-1]])/2.0) // delta to neighboring y
    } else {
        ((lat_lon_grid.lat_grid[[x,y+1]] - lat_lon_grid.lat_grid[[x,y]])/2.0, (lat_lon_grid.lon_grid[[x,y+1]] - lat_lon_grid.lon_grid[[x,y]])/2.0) // delta to neighboring y
    };

    let nw = LatLon{lat_deg: lat_lon_grid.lat_grid[[x,y]] - dx.0 - dy.0, lon_deg: lat_lon_grid.lon_grid[[x,y]] - dx.1 - dy.1}; // -dx + dy
    let sw = LatLon{lat_deg: lat_lon_grid.lat_grid[[x,y]] - dx.0 + dy.0, lon_deg: lat_lon_grid.lon_grid[[x,y]] - dx.1 + dy.1}; // - dx - dy
    let ne = LatLon{lat_deg: lat_lon_grid.lat_grid[[x,y]] + dx.0 - dy.0, lon_deg: lat_lon_grid.lon_grid[[x,y]] + dx.1 - dy.1}; // +dx + dy
    let se = LatLon{lat_deg: lat_lon_grid.lat_grid[[x,y]] + dx.0 + dy.0, lon_deg: lat_lon_grid.lon_grid[[x,y]] + dx.1 + dy.1}; // +dx -dy
    Ok(GoesRBoundingBox {nw:nw, sw:sw, ne:ne, se:se})
}

pub fn get_bounds_vector(x_vals: &Vec<usize>, y_vals: &Vec<usize>, lat_lon_grid: &LatLonGrid) -> Result<Vec<GoesRBoundingBox>> {
    let mut bounds = Vec::<GoesRBoundingBox>::new();
    for i in 0..x_vals.len() {
        bounds.push(get_bounds(x_vals[i], y_vals[i], &lat_lon_grid)?);
    }
    Ok(bounds)
}

pub fn get_lat_lons(x_vals: &Vec<usize>, y_vals: &Vec<usize>, lat_lon_grid: &LatLonGrid) -> Result<Vec<LatLon>> {
    let mut lat_lons: Vec<LatLon> = vec![];
    for i in 0..x_vals.len() {
        lat_lons.push(LatLon{lat_deg: lat_lon_grid.lat_grid[[x_vals[i], y_vals[i]]],
                            lon_deg: lat_lon_grid.lon_grid[[x_vals[i], y_vals[i]]] 
                        });
    }
    Ok(lat_lons)
}

pub fn get_lat_lon_grid(data: &GoesRData) -> Result<LatLonGrid>{
    // adapted from https://www.star.nesdis.noaa.gov/atmospheric-composition-training/python_abi_lat_lon.php
    // get x, y data - need add_offset and scale_factor
    let area_file = format!("NETCDF:{:?}:Area", data.file);
    let ds = Dataset::open(area_file)?;
    let proj = get_projection(&ds)?;
    let grid_params = get_xy_grid_params(&ds)?;
    let size = ds.raster_size();
    // set up x - for 0..2500 do add_offset+scale_factor*i, same for y
    let mut x: Vec<f64> = vec![];
    let mut y: Vec<f64> = vec![];
    for i in 0..size.0 {
        x.push(grid_params.x_start + (i as f64 *grid_params.x_scale));
    }
    for i in 0..size.1 {
        y.push(grid_params.y_start + (i as f64 *grid_params.y_scale));
    }
    let x_array = Array::from_vec(x);
    let y_array = Array::from_vec(y);
    let grids = meshgrid(&vec![x_array, y_array], Indexing::Xy).unwrap();
    let x_grid = &grids[0];
    let y_grid = &grids[1];
    let lon_origin = proj.longitude_of_projection_origin;
    let h = proj.perspective_point_height+proj.semi_major_axis;
    let r_eq = proj.semi_major_axis;
    let r_pol = proj.semi_minor_axis;
    // Equations to calculate latitude and longitude
    let lambda_0 = (lon_origin*PI)/180.0;
    // // let sin_x = x_grid.mapv(|x| f64::sin(x).powf(2.0));
    // // let cos_x = x_grid.mapv(|x| f64::cos(x).powf(2.0));
    // // let cos_y = y_grid.mapv(|y| f64::cos(y).powf(2.0));
    // // let sin_y = y_grid.mapv(|y| f64::sin(y).powf(2.0));
    // let a_var = &x_grid.mapv(|x| f64::sin(x).powf(2.0)) + (&x_grid.mapv(|x| f64::cos(x).powf(2.0))*((&y_grid.mapv(|y| f64::cos(y).powf(2.0)))+(((r_eq*r_eq)/(r_pol*r_pol))*(&y_grid.mapv(|y| f64::sin(y).powf(2.0))))));
    // let b_var = -2.0*h*&x_grid.mapv(|x| f64::cos(x)) * &y_grid.mapv(|y| f64::cos(y)); 
    // let c_var = (&h.powf(2.0))-(&r_eq.powf(2.0));
    // let r_s = (-1.0*&b_var - ((&b_var.mapv(|x| x.powf(2.0)))-(4.0 * &a_var * c_var)).mapv(|x| f64::sqrt(x)))/(2.0*&a_var);
    // let s_x = &r_s * &x_grid.mapv(|x| f64::cos(x)) * &y_grid.mapv(|y| f64::cos(y)); 
    // let s_y = - &r_s * &x_grid.mapv(|x| f64::sin(x));
    // let s_z = &r_s * &x_grid.mapv(|x| f64::cos(x)) * &y_grid.mapv(|y| f64::sin(y));
    // chatgpt optimization

    // Precalculate sine and cosine values
    let sin_x = x_grid.mapv(f64::sin);
    let cos_x = x_grid.mapv(f64::cos);
    let sin_y = y_grid.mapv(f64::sin);
    let cos_y = y_grid.mapv(f64::cos);

    let sin_x_squared = sin_x.mapv(|x| x.powf(2.0));
    let cos_x_squared = cos_x.mapv(|x| x.powf(2.0));
    let cos_y_squared = cos_y.mapv(|y| y.powf(2.0));
    let sin_y_squared = sin_y.mapv(|y| y.powf(2.0));

    // Compute intermediate variables
    let a_var = &sin_x_squared + (&cos_x_squared * (&cos_y_squared + ((r_eq * r_eq) / (r_pol * r_pol)) * &sin_y_squared));
    let b_var = -2.0 * h * &cos_x * &cos_y;
    let c_var = h.powf(2.0) - r_eq.powf(2.0);

    // Compute r_s
    let discriminant = &b_var.mapv(|x| x.powf(2.0)) - (4.0 * &a_var * c_var);
    let r_s = (-&b_var - discriminant.mapv(f64::sqrt)) / (2.0 * &a_var);

    // Compute s_x, s_y, s_z
    let s_x = &r_s * &cos_x * &cos_y;
    let s_y = -&r_s * &sin_x;
    let s_z = &r_s * &cos_x * &sin_y;

    // 2 2d arrays, one of x values, one of y values, then calculate the things according to https://www.star.nesdis.noaa.gov/atmospheric-composition-training/python_abi_lat_lon.php
    // calculate
    let abi_lat = (180.0/PI)*((((&r_eq * &r_eq)/(&r_pol * &r_pol))*(&s_z/((&s_x.mapv(|x| (&h - x).powf(2.0)))+(&s_y.mapv(|x| x.powf(2.0)))).mapv(|x| f64::sqrt(x))).mapv(|x| f64::atan(x))));
    let abi_lon =  (&s_y / (&s_x.mapv(|x| &h - x))).mapv(|x| (&lambda_0 -f64::atan(x)))*(180.0/PI);
    // return an array of latlons
    let max_value = abi_lat.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    println!("Lat Maximum value: {}", max_value);
    // Get the minimum value
    let min_value = abi_lat.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    println!("Lat Minimum value: {}", min_value);
    let max_value = abi_lon.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    println!("Lon Maximum value: {}", max_value);
    // Get the minimum value
    let min_value = abi_lon.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    println!("Lon Minimum value: {}", min_value);
    // let lat_lons = abi_lat.iter().zip(abi_lon.iter()).map(|(lat, lon)| LatLon{lat_deg:*lat, lon_deg:*lon}).collect::<Array2<LatLon>>();
    // println!("lat lon grid {:?}", lat_lons.dim());
    Ok(LatLonGrid{lat_grid:abi_lat,lon_grid: abi_lon})
}