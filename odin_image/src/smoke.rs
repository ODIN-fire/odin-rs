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

//! module to detect smoke patterns in images

use image::{DynamicImage, GenericImageView, SubImage};
use crate::{errors::{OdinImageError,Result}, get_grid_dim, tiled_mean_gw, tile_data::{SearchDir, TileData}, Mask};

pub struct SmokeDiff {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub g_diff: f32,
    pub w_diff: f32
}
/*
pub fn get_hsv_smoke_diff (
    img1: &DynamicImage, img2: &DynamicImage, 
    tile_width: u32, tile_height: u32, fractional_tiles: bool, mask: Option<&Mask>,
    min_valid: f32, max_s_diff: f32, min_v_diff: f32, v_saturation: f32,
    horizon: &[u32]
)->Result<Vec<SmokeDiff>> 
{
    if img1.dimensions() != img2.dimensions() { return Err( OdinImageError::InvalidDimensions("image dimensions differ".into())); }
    if horizon.len() != img1.width() as usize { return Err( OdinImageError::InvalidDimensions("incompatible horizon length".into())); }

    let valid_pixel_predicate = |x: u32, y: u32, hsv: &(f32,f32,f32)| {
        (y >= horizon[x as usize]) && hsv.2 < v_saturation
    };

    let (h1,s1,v1) = get_mean_hsv_tile_data( &img1, tile_width, tile_height, fractional_tiles, mask, min_valid, &valid_pixel_predicate)?;
    let (h2,s2,v2) = get_mean_hsv_tile_data( &img2, tile_width, tile_height, fractional_tiles, mask, min_valid, &valid_pixel_predicate)?;

    let s_diff = s2.diff(&s1)?;
    let v_diff = v2.diff(&v1)?;
    let mut v_matches = v_diff.greater_equal_cells( min_v_diff, SearchDir::ColMajor);

    v_matches.retain( |p| s_diff.get( p.0, p.1) < max_s_diff); // saturation has to go down for valid smoke pixels

    let smoke_data = v_matches.into_iter().map( |(i,j)| {
        let x = i as u32 * tile_width;
        let y = j as u32 * tile_height;
        let width = tile_width;
        let height = tile_height;
        let s_diff = s_diff.get(i,j);
        let v_diff = v_diff.get( i,j);
        SmokeDiff{x,y,width,height,s_diff,v_diff}
    }).collect();

    Ok( smoke_data )
}
*/

/// white-ness factor has to increase (brightness)
/// gray-ness factor has to increase
pub fn get_gw_smoke_diff (
    img1: &DynamicImage, img2: &DynamicImage, 
    tile_width: u32, tile_height: u32, fractional_tiles: bool, mask: Option<&Mask>,
    min_valid: f32, min_g_diff: f32, min_w_diff: f32, w_saturation: f32,
    horizon: &[u32]
)->Result<Vec<SmokeDiff>> 
{
    if img1.dimensions() != img2.dimensions() { return Err( OdinImageError::InvalidDimensions("image dimensions differ".into())); }
    if horizon.len() != img1.width() as usize { return Err( OdinImageError::InvalidDimensions("incompatible horizon length".into())); }

    let valid_pixel_predicate = |x: u32, y: u32, gw: &(f32,f32)| {
        (y >= horizon[x as usize]) && gw.1 < w_saturation
    };

    let (g1,w1) = tiled_mean_gw( &img1, tile_width, tile_height, fractional_tiles, mask, min_valid, &valid_pixel_predicate)?;
    let (g2,w2) = tiled_mean_gw( &img2, tile_width, tile_height, fractional_tiles, mask, min_valid, &valid_pixel_predicate)?;

    let g_diff = g2.diff(&g1)?;
    let w_diff = w2.diff(&w1)?;

    let mut candidates = w_diff.greater_equal_cells( min_w_diff, SearchDir::ColMajor);

    candidates.retain( |p| g_diff.get( p.0, p.1) >= min_g_diff); // gray-ness also has to go up (but less sensitive)

    let smoke_data = candidates.into_iter().map( |(i,j)| {
        let x = i as u32 * tile_width;
        let y = j as u32 * tile_height;
        let width = tile_width;
        let height = tile_height;
        let g_diff = g_diff.get(i,j);
        let w_diff = w_diff.get( i,j);
        SmokeDiff{x,y,width,height,g_diff,w_diff}
    }).collect();

    Ok( smoke_data )
}