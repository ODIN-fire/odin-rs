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

/// classic image processing module of ODIN
/// this mostly wraps and extends the external 'image' crate (and related)
 
use std::{fs::File, io::{Read, Write}, ops::{Deref,Add, Div, Mul, Sub}, path::{Path,PathBuf}};
use flate2::{write::{GzEncoder,DeflateEncoder}, Compression};
use image::{self, imageops::{blur_advanced, FilterType, GaussianBlurParameters}, DynamicImage, EncodableLayout, GenericImage, GenericImageView, GrayImage, ImageBuffer, Luma, Pixel, Primitive, Rgb, RgbImage, Rgba, RgbaImage, SubImage
};
use image_compare::{self, Algorithm, CompareError, Metric, Similarity};
//use miniz_oxide::deflate::compress_to_vec;
use num::{Zero,Bounded};
use linreg::linear_regression;
use ndarray::Array2;
use tiff::encoder::{colortype, Compression as TiffCompression, DeflateLevel, TiffEncoder, TiffValue};
use imageproc::{drawing::{draw_hollow_rect_mut, draw_text_mut},rect::Rect};
use ab_glyph::{Font,FontVec,PxScale};
use edge_detection::{canny,Detection,Edge};
use trait_set::trait_set;
use lazy_static::lazy_static;
use serde_json;
use odin_common::{pow2,sqrt,fs::{extension,filepath_contents}};
use odin_build::pkg_data_dir;

mod mask;
pub use mask::Mask;

mod errors;
pub use errors::{Result,OdinImageError};

mod loess;
pub use loess::LinearLoess;

mod tile_data;
pub use tile_data::TileData;

pub mod smoke;

pub struct Stats<T> {
    pub min: T,
    pub max: T,
    pub mean: f64,
    pub variance: f64,

    pub s: f64,
    pub n: usize
}

impl <T> Stats<T> 
    where T: Add<T,Output=T> + Sub<T,Output=T> + Div<T,Output=T> + Mul<T,Output=T> + 
             Bounded + PartialOrd + PartialEq + Zero + Into<f64> + Copy
{
    pub fn new ()->Self {
        let min = T::max_value();
        let max = T::min_value();
        let mean: f64 = 0.0;
        let variance: f64 = 0.0;

        Stats{min,max,mean,variance, s: 0.0, n: 0}
    }

    pub fn add (&mut self, v: T) {
        self.n += 1;

        if v < self.min { self.min = v }
        if v > self.max { self.max = v }

        let prev_mean = self.mean;
        let v: f64 = v.into();
        let n = self.n as f64;

        self.mean = (v + (n * prev_mean) - prev_mean) / n;
        self.s = self.s + (v - prev_mean) * (v - self.mean);
        self.variance = self.s / n;
    }
}

const R: usize = 0;
const G: usize = 1;
const B: usize = 2;

pub type GrayImage8 = ImageBuffer<Luma<u8>, Vec<u8>>;
pub type GrayImage16 = ImageBuffer<Luma<u16>, Vec<u16>>;
pub type GrayImage32f = ImageBuffer<Luma<f32>, Vec<f32>>;

pub fn check_equal_dimensions (img1: &DynamicImage, img2: &DynamicImage)->Result<()> {
    if img1.dimensions() != img2.dimensions() {
        Err( OdinImageError::InvalidDimensions("image dimensions differ".into()) )
    } else {
        Ok(())
    }
}

pub fn check_equal_regions (sub1: &SubImage<&DynamicImage>, sub2: &SubImage<&DynamicImage>)->Result<()> {
    if sub1.offsets() != sub2.offsets() {
        Err( OdinImageError::InvalidDimensions("sub image offsets differ".into()) )
    } else {
       if sub1.dimensions() != sub2.dimensions() {
            Err( OdinImageError::InvalidDimensions("sub image dimensions differ".into()) )
       } else {
            Ok(())
       }
    }
}

pub fn open_diff_image_pair<P> (path1: P, path2: P)->Result<(DynamicImage,DynamicImage)> where P: AsRef<Path> {
    let img1 = image::open(path1)?;
    let img2 = image::open(path2)?;
    if img1.dimensions() == img2.dimensions() { 
        Ok( (img1, img2) )
    } else {
        Err( OdinImageError::InvalidDimensions("images have different dimensions".into()))
    }
}


pub fn get_hex_rgb (hex_color: &str)->[u8;3] {
    let v = u32::from_str_radix( hex_color, 16).expect("invalid hex color spec");

    let r = (v >> 16) as u8;
    let g = (v >> 8 & 0xff) as u8;
    let b = (v & 0xff) as u8;

    [r, g, b]
}

/* #region histogram equalization ************************************************************************************/

pub fn rgb_equalize<P,Q> (in_path: P, out_path: Q)->Result<()> where P: AsRef<Path>, Q: AsRef<Path> {
    let input_img = image::open( in_path.as_ref())?;
    let output_img = rgb_histogram_equalize( &input_img);
    Ok( output_img.save( out_path.as_ref())? )
}

pub fn rgb_histogram_equalize (img: &DynamicImage) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let rgb_img = img.to_rgb8();
    let (width, height) = rgb_img.dimensions();
    let total_pixels = (width * height);

    // Process each channel independently
    let mut output: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(width, height);

    let (r_lut,g_lut,b_lut) = compute_u8_equalization_luts(&rgb_img);

    for (x, y, pixel) in rgb_img.enumerate_pixels() {
        let mut out_pixel = *output.get_pixel(x, y);
        out_pixel[R] = r_lut[pixel[R] as usize];
        out_pixel[G] = g_lut[pixel[G] as usize];
        out_pixel[B] = g_lut[pixel[B] as usize];
        output.put_pixel( x, y, out_pixel);
    }

    output
}

/// get the lookup tables for u8 RGB channel equalization
fn compute_u8_equalization_luts (rgb_img: &ImageBuffer<Rgb<u8>, Vec<u8>>) -> ([u8; 256],[u8;256],[u8;256]) {
    let (width, height) = rgb_img.dimensions();
    let n_pixels = width * height;

    let mut r_hist = [0u32; 256];
    let mut g_hist = [0u32; 256];
    let mut b_hist = [0u32; 256];

    for pixel in rgb_img.pixels() {
        r_hist[pixel[R] as usize] += 1;
        g_hist[pixel[G] as usize] += 1;
        b_hist[pixel[B] as usize] += 1;
    }

    let r_lut = compute_lut( &r_hist, n_pixels);
    let g_lut = compute_lut( &g_hist, n_pixels);
    let b_lut = compute_lut( &b_hist, n_pixels);

    (r_lut, g_lut, b_lut)
}

fn compute_lut (histogram: &[u32;256], n_pixels: u32)-> [u8;256] {
    let n_pixels = n_pixels as f32;

    let mut cdf = [0u32; 256]; // the cumulative distribution function for this histogram
    cdf[0] = histogram[0];
    for i in 1..256 { cdf[i] = cdf[i - 1] + histogram[i]; }

    let mut i = 0;
    while i<255 && cdf[i] == 0 { i +=1; }
    let cdf_min = cdf[i];

    //let cdf_min = *cdf.iter().find(|&&x| x > 0).unwrap_or(&0);

    let mut lut = [0u8; 256]; // the lookup table to produce
    for i in 0..256 {
        if cdf[i] > 0 {
            lut[i] = (((cdf[i] - cdf_min) as f32 / (n_pixels - cdf_min as f32)) * 255.0).round() as u8;
        }
    }

    lut
}

/* #endregion histogram equalization */


pub fn get_rgb_diff_image<P,Q,R> (in_path1: P, in_path2: Q, out_path: R)->Result<()> where P: AsRef<Path>, Q: AsRef<Path>, R: AsRef<Path> {
    let input_img1 = image::open( in_path1.as_ref())?;
    let input_img2 = image::open( in_path2.as_ref())?;

    let res = rgb_hybrid_compare( &input_img1, &input_img2)?;
    let diff_img = res.image.to_color_map();
    Ok( diff_img.save(out_path.as_ref())? )
}

pub fn rgb_hybrid_compare (img1: &DynamicImage, img2: &DynamicImage)->Result<Similarity> {
    let rgb_img1 = img1.as_rgb8().ok_or( OdinImageError::InvalidImageFormat("not an RGB image".into()))?;
    let rgb_img2 = img2.as_rgb8().ok_or( OdinImageError::InvalidImageFormat("not an RGB image".into()))?;

    Ok( image_compare::rgb_hybrid_compare( rgb_img1, rgb_img2)? )
}

pub fn gray_structure_compare (img1: &DynamicImage, img2: &DynamicImage, algorithm: Algorithm)->Result<Similarity> {
    let gray_img1 = img1.to_luma8();
    let gray_img2 = img2.to_luma8();

    Ok( image_compare::gray_similarity_structure( &algorithm, &gray_img1, &gray_img2)? )
}

pub fn gray_histogram_compare (img1: &DynamicImage, img2: &DynamicImage, metric: Metric)->Result<f64> {
    let gray_img1 = img1.to_luma8();
    let gray_img2 = img2.to_luma8();

    Ok( image_compare::gray_similarity_histogram( metric, &gray_img1, &gray_img2)? )
}

pub fn create_luma8_image<P,Q> (in_path: P, out_path: Q)->Result<()> where P: AsRef<Path>, Q: AsRef<Path> {
    let input_img = image::open( in_path.as_ref())?;
    let out_img = to_luma8( &input_img)?;
    Ok( out_img.save( out_path.as_ref())? )
}

pub fn to_luma8 (img: &DynamicImage)->Result<GrayImage> {
    Ok( img.to_luma8() )
}

pub fn create_luma16_image<P,Q> (in_path: P, out_path: Q)->Result<()> where P: AsRef<Path>, Q: AsRef<Path> {
    let input_img = image::open( in_path.as_ref())?;
    let out_img = to_luma16( &input_img)?;
    Ok( out_img.save( out_path.as_ref())? )
}

pub fn to_luma16 (img: &DynamicImage)->Result<GrayImage16> {
    Ok( img.to_luma16() )
}

pub fn create_luma32f_image<P,Q> (in_path: P, out_path: Q)->Result<()> where P: AsRef<Path>, Q: AsRef<Path> {
    let ext = extension( &out_path);
    if ext.is_none() || !ext.unwrap().ends_with("tif") {
        return Err( OdinImageError::IllegalArgument(format!("f32 grayscale has to be stored as *.tif file")));
    }

    let input_img = image::open( in_path.as_ref())?;
    let (w,h) = input_img.dimensions();
    let out_img = to_luma32f( &input_img)?;

    let mut out_file: File = File::create_new( out_path.as_ref())?;
    let mut tiff = TiffEncoder::new(&mut out_file)?.with_compression( TiffCompression::Deflate(DeflateLevel::Best));
    tiff.write_image::<colortype::Gray32Float>( w, h, out_img.as_raw().as_ref())?;
    Ok( () )
}

pub fn to_luma32f (img: &DynamicImage)->Result<GrayImage32f> {
    Ok( img.to_luma32f() )
}

/* #region font resources ************************************************************************************************/

const DEFAULT_FONT_NAME: &'static str = "DejaVuSansMono.ttf"; // make sure this is in ODIN_ROOT/data/odin_image

pub fn font_path(font_name: &str)->PathBuf {
    pkg_data_dir!().join( font_name)
}

pub fn load_font (font_name: &str)->Result<FontVec> {
    let path = font_path( font_name);
    if !path.is_file() {
        Err( OdinImageError::OpFailed(format!("font not found: {}", font_name)) )

    } else {
        let data = filepath_contents( &path)?;
        Ok( FontVec::try_from_vec( data)? )
    }
}

pub fn load_default_font ()->Result<FontVec> {
    load_font( DEFAULT_FONT_NAME)
}

/* #endregion font resources */

/* #region grid/tile processing functions ********************************************************************************/

pub fn get_tile_size (img: &DynamicImage, nx: usize, ny: usize)->(u32,u32) {
    let (w,h) = img.dimensions();
    let tile_width: u32 = ((w as f64) / (nx as f64)).ceil() as u32;
    let tile_height: u32 = ((h as f64) / (ny as f64)).ceil() as u32;

    (tile_width, tile_height)
}

pub fn get_dominant_tile_size (img: &DynamicImage, n: usize, is_horizontal: bool, keep_aspect_ratio: bool)->(u32,u32) {
    if keep_aspect_ratio {
        get_aspect_ratio_tile_size( &img, n, is_horizontal)
    } else {
        get_square_tile_size( &img, n, is_horizontal)
    }
}

pub fn get_aspect_ratio_tile_size (img: &DynamicImage, n: usize, is_horizontal: bool)->(u32,u32) {
    let (w,h) = img.dimensions();
    let aspect_ratio = w as f32 / h as f32;

    if is_horizontal {
        let tile_width = ((w as f64) / (n as f64)).floor() as u32;
        let tile_height: u32 = ((tile_width as f32 / aspect_ratio).floor()) as u32;
        (tile_width, tile_height)
    } else {
        let tile_height: u32 = ((h as f64) / (n as f64)).floor() as u32;
        let tile_width: u32 = (tile_height as f32 * aspect_ratio).floor() as u32;
        (tile_width,tile_height)
    }
}

pub fn get_square_tile_size (img: &DynamicImage, n: usize, is_horizontal: bool)->(u32,u32) {
    let (w,h) = img.dimensions();

    if is_horizontal {
        let tile_width = ((w as f64) / (n as f64)).floor() as u32;
        (tile_width, tile_width)
    } else {
        let tile_height: u32 = ((h as f64) / (n as f64)).floor() as u32;
        (tile_height,tile_height)
    }
}

pub fn get_grid_dim (img: &DynamicImage, tile_width: u32, tile_height: u32, fractional_tiles: bool)->(usize,usize) {
    let (w,h) = img.dimensions();
    let mut nx = (w/tile_width) as usize;
    let mut ny = (h/tile_height) as usize;

    if fractional_tiles {
        if (w % tile_width) > 0 { nx += 1 }
        if (h % tile_height) > 0 { ny += 1 }
    }  

    (nx,ny)
}

pub fn process_subimage_tiles_mut<F> (img: &mut DynamicImage, tile_width: u32, tile_height: u32, fractional_tiles: bool, mask: Option<&Mask>, mut f: F)->Result<()> 
    where F: FnMut(&mut SubImage<&mut DynamicImage>, (usize,usize))
{
    let (w,h) = img.dimensions();
    let (nx,ny) = get_grid_dim( img, tile_width, tile_height, fractional_tiles);

    for j in 0..ny {
        let y = j as u32 * tile_height;
        let th = tile_height.min( h - y);
        for i in 0..nx {
            let x = i as u32 * tile_width;
            let tw = tile_width.min( w - x);
            if !is_masked( i,j, &mask) { f( &mut img.sub_image( x, y, tw, th), (i,j)); }
        }
    }

    Ok(())
}


pub fn process_subimage_tiles<F> (img: &DynamicImage, tile_width: u32, tile_height: u32, fractional_tiles: bool, mask: Option<&Mask>, mut f: F)->Result<()> 
    where F: FnMut(&SubImage<&DynamicImage>, (usize,usize))
{
    let (w,h) = img.dimensions();
    let (nx,ny) = get_grid_dim( img, tile_width, tile_height, fractional_tiles);

    for j in 0..ny {
        let y = j as u32 * tile_height;
        let th = tile_height.min( h - y);
        for i in 0..nx {
            let x = i as u32 * tile_width;
            let tw = tile_width.min( w - x);
            if !is_masked( i,j, &mask) { f( &img.view( x, y, tw, th), (i,j)); }
        }
    }

    Ok(())
}

#[inline(always)]
pub fn is_masked (x: usize, y: usize, mask: &Option<&Mask>)->bool {
    mask.is_some() && !mask.unwrap().get( x,y)
}

pub fn get_tile_mut (img: &mut DynamicImage, tile_width: u32, tile_height: u32, i: usize, j: usize)->Result<SubImage<&mut DynamicImage>> {
    let (w,h) = img.dimensions();
    let x = tile_width * i as u32;
    let y = tile_height * j as u32;
    if x >= w || y >= h { return Err( OdinImageError::IllegalArgument("tile origin outside image bounds".into())); }

    let tw = tile_width.min( w - x);
    let th = tile_height.min( h - y);
    Ok( img.sub_image( x, y, tw, th) )
}

pub fn get_tile (img: &DynamicImage, tile_width: u32, tile_height: u32, i: usize, j: usize)->Result<SubImage<&DynamicImage>> {
    let (w,h) = img.dimensions();
    let x = tile_width * i as u32;
    let y = tile_height * j as u32;
    if x >= w || y >= h { return Err( OdinImageError::IllegalArgument("tile origin outside image bounds".into())); }

    let tw = tile_width.min( w - x);
    let th = tile_height.min( h - y);
    Ok( img.view( x, y, tw, th) )
}

pub fn process_tile<F> (img: &DynamicImage, tile_width: u32, tile_height: u32, i: usize, j: usize, mut f: F)->Result<()>
    where F: FnMut(&SubImage<&DynamicImage>)
{
    let sub = get_tile( img, tile_width, tile_height, i, j)?;
    f( &sub);
    Ok(())
}

pub fn process_tile_mut<F> (img: &mut DynamicImage, tile_width: u32, tile_height: u32, i: usize, j: usize, mut f: F)->Result<()>
    where F: FnMut(&SubImage<&mut DynamicImage>)
{
    let sub = get_tile_mut( img, tile_width, tile_height, i, j)?;
    f( &sub);
    Ok(())
}

pub fn draw_tile_grid (img: &mut RgbImage, tile_width: u32, tile_height: u32, pt_size: f32, color: Rgb<u8>, mask: Option<&Mask>)->Result<()> {
    let (w,h) = img.dimensions();

    let nx = ((w-1) / tile_width) as usize + 1;
    let ny = ((h-1) / tile_height) as usize + 1;

    let font = load_default_font()?;
    let scale = font.pt_to_px_scale(pt_size).ok_or( OdinImageError::IllegalArgument("invalid font pt size".to_string()))?;
    let scale_px = font.pt_to_px_scale(pt_size - 1.5).ok_or( OdinImageError::IllegalArgument("invalid font pt size".to_string()))?;

    let mut x: u32 = 0;
    let mut y: u32 = 0;

    for j in 0..ny {
        for i in 0..nx {
            if mask.is_none() || mask.unwrap().get(i,j) {
                let rect = Rect::at(x as i32, y as i32 + 1).of_size( tile_width, tile_height); // ?? y+1 bug in imageproc ??
                draw_hollow_rect_mut( img, rect, color);

                // pixel coordinates
                let text = format!("{x},{y}");
                draw_text_mut( img, color, x as i32 + 3, y as i32 + (scale_px.y/2.0) as i32, scale_px, &font, text.as_str());

                // tile indices
                let text = format!("{i},{j}");
                draw_text_mut( img, color, x as i32 + 3, (y + tile_height) as i32 - scale.y as i32 - 3, scale, &font, text.as_str());
            }

            x += tile_width;
        }
        x = 0;
        y += tile_height;
    }

    Ok(())
}

/// set every pixel above horizon line to black
pub fn blackout_sky (img: &mut DynamicImage, horizon: &[u32])->Result<()> {
    let (w,h) = img.dimensions();
    if w > horizon.len() as u32 { return Err( OdinImageError::InvalidDimensions("horizon and image width differ".into()) )}

    for x in 0..w {
        for y in 0..h.min(horizon[x as usize]) {
            img.put_pixel( x, y, Rgba([0,0,0,0]));
        }
    }

    Ok(())
}

/// set every pixel below the horizon line to black
pub fn blackout_terrain (img: &mut DynamicImage, horizon: &[u32])->Result<()> {
    let (w,h) = img.dimensions();
    if w != horizon.len() as u32 { return Err( OdinImageError::InvalidDimensions("horizon and image width differ".into()) )}

    for x in 0..w {
        for y in horizon[x as usize].min(h)..h {
            img.put_pixel( x, y, Rgba([0,0,0,0]));
        }
    }

    Ok(())
}

pub fn blackout_above (img: &mut DynamicImage, y_cut: u32)->Result<()> {
    let (w,h) = img.dimensions();
    if y_cut >= h { return Err( OdinImageError::IllegalArgument("y outside limts".into())) }

    for y in 0..=y_cut {
        for x in 0..w {
            img.put_pixel( x, y, Rgba([0,0,0,0]));
        }
    }

    Ok(())
}

pub fn blackout_below (img: &mut DynamicImage, y_cut: u32)->Result<()> {
    let (w,h) = img.dimensions();
    if y_cut >= h { return Err( OdinImageError::IllegalArgument("y outside limts".into())) }

    for y in y_cut..h {
        for x in 0..w {
            img.put_pixel( x, y, Rgba([0,0,0,0]));
        }
    }

    Ok(())
}

/* #endregion grid/tile processing functions */

/* #region focus *******************************************************************************************/

/// average horizontal Brenner focus of a sub image (computed from Luma channel)
/// see https://cs.uwaterloo.ca/~vanbeek/Publications/spie2014.pdf
pub fn avg_horizontal_brenner_focus (img: &SubImage<&mut DynamicImage>)->f32 {
    let (w,h) = img.dimensions();

    let mut focus: f64 = 0.0;

    for x in 0..w {
        for y in 0..h-2 {
            let l = img.get_pixel(x, y).to_luma().0[0];  // TODO - does this incur cost if the image is already in Luma ? 
            let l2 = img.get_pixel(x,y+2).to_luma().0[0]; 
            let diff = l2 - l;
            let diff2 = diff*diff;

            focus += diff2 as f64;
        }
    }

    (focus / (w * (h-2)) as f64) as f32  // average focus 
}

/* #endregion focus */

/* #region fractal dimension ***********************************************************************************************/

/// this implements estimation of a fractal dimension metric according to "Smoke detection in images through fractal dimension-based binary classification"
/// Javier Del-Pozo-Velázquez et al, Digital Signal Processing Vol 166 (Nov 2025) 
/// https://www.sciencedirect.com/science/article/pii/S1051200425003689
/// While it does show enough sensitivity to smoke against ground for differential analysis it does not handle sky well (the oauthors just cropped sky) 
/// hence this should be used with a horizon line that removes the sky
/// Note this requires a regular ImageBuffer and does not work with SubImages since we need a contiguous data array for compression efficiency.
/// This means this method is both CPU and memory intensive
pub fn fractal_dim (img: &DynamicImage, s_min: f32)->Result<f32> {
    if s_min <= 0.0 || s_min >= 1.0 { return Err( OdinImageError::IllegalArgument(format!("s_min out of range {}", s_min))) }

    let (width,height) = img.dimensions();

    let n = ((1.0 - s_min) / 0.1) as usize + 1;
    let mut scales: Vec<f32> = Vec::with_capacity( n);
    let mut sizes: Vec<f32> = Vec::with_capacity( n);

    let mut s: f32 = s_min;
    while s < 1.0 {
        let w = ((width as f32) * s) as u32;
        let h = ((height as f32) * s) as u32;
        let s_img = img.resize(w, h, FilterType::CatmullRom);
        let sz = compressed_size( &s_img);

        scales.push( (s * 10 as f32).log2());
        sizes.push( (sz as f32).log2()); 

        s += 0.1;
    }

    scales.push( 10.0_f32.log2());
    sizes.push( (compressed_size( img) as f32).log2());

    let (slope,intercept) = linear_regression( scales.as_slice(), sizes.as_slice()).map_err( |e| OdinImageError::OpFailed(e.to_string()))?;

    Ok(slope)
}

/// a dummy writer that just keeps track of how many bytes have been written to it
struct NullWriter {
    n_bytes: usize
}

impl NullWriter {
    pub fn new()->Self {
        NullWriter { n_bytes: 0 }
    }
    pub fn len(&self)->usize {
        self.n_bytes
    }
}

impl Write for NullWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let len = buf.len();
        self.n_bytes += len;
        Ok(len)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // nothing to do - we don't physically write anything
        Ok(())
    }
}

pub fn fractal_dim_of_tile (sub: &SubImage<&mut DynamicImage>, s_min: f32)->Result<f32> {
    let img: DynamicImage = sub.to_image().into(); // get contiguous pixel data
    fractal_dim( &img, s_min)
}

pub fn fractal_dim_of_luma8_tile (sub: &SubImage<&mut DynamicImage>, s_min: f32)->Result<f32> {
    let img: DynamicImage = sub.to_image().into(); // get contiguous pixel data
    let luma8_img = DynamicImage::ImageLuma8(img.into_luma8()); // map into luma8 to reduce RGB channel noise
    fractal_dim( &luma8_img, s_min)
}

pub fn compressed_size (img: &DynamicImage)->usize {
    //let mut enc = GzEncoder::new( NullWriter::new(), Compression::best());
    let mut enc = DeflateEncoder::new( NullWriter::new(), Compression::best());
    enc.write_all( img.as_bytes());
    enc.finish().unwrap().len()

    //compress_to_vec( img.as_bytes(), 9).len()
}

/// note the results differ from compressing sub.as_bytes(), which also includes the (empty) alpha channel
pub fn compressed_sub_size (sub: &SubImage<&DynamicImage>) -> usize {
    let (w,h) = sub.dimensions();
    let mut scanline: Vec<u8> = Vec::with_capacity(w as usize * 3);
    let mut enc = DeflateEncoder::new( NullWriter::new(), Compression::best());

    for y in 0..h {
        for x in 0..w {
            let [r,g,b,_] = sub.get_pixel(x, y).0;
            scanline.push(r);
            scanline.push(g);
            scanline.push(b);
        }
        enc.write( &scanline);
        scanline.clear();
    }

    enc.finish().unwrap().len()
}

/// note this is only a coarse approximation of difference that requires tiles to be large enough to compare complexity
/// it can be used to detect changes in uniform color areas
/// othewise the (significantly more expensive) fractal dimension analysis is a better metric for complexity
pub fn get_tiled_comp (img: &DynamicImage, tile_width: u32, tile_height: u32, fractional_tiles: bool, mask: Option<&Mask>) -> Result<TileData<u32>>  
{
    let (nx, ny) = get_grid_dim( img, tile_width, tile_height, fractional_tiles);
    let mut comp_data: TileData<u32> = TileData::new( nx, ny);

    let mut compute_comp_tile_data = |sub: &SubImage<&DynamicImage>, p: (usize,usize)| {
        let size = compressed_sub_size(sub);
        comp_data.set( p.0, p.1, size as u32);
    };

    process_subimage_tiles( img, tile_width, tile_height, fractional_tiles, mask, compute_comp_tile_data)?;
    Ok( comp_data )
}

/* #endregion fractal dimension */

/* #region edge/horizon detection *******************************************************************************/

pub fn canny_horizon_line (img: &DynamicImage, offset: u32, sigma: f32, strong: f32, weak: f32, threshold: f32, loess_width: usize)->Result<Vec<u32>> {
    let luma8_img = if offset > 0 {
        let cropped_img = img.crop_imm(0, offset, img.width(), img.height() - offset);
        cropped_img.to_luma8()
    } else {
        img.to_luma8()
    };

    let detection = canny( luma8_img, sigma, strong, weak); // the multi-pass Canny edge detection

    let w = detection.width();
    let h = detection.height();

    let mut horizon: Vec<u32> = vec![offset; w];

    'outer: for x in 0..w {
        for y in 0..h {
            if detection[(x,y)].magnitude() > threshold {
                horizon[x] = y as u32 + offset;
                continue 'outer;
            }
        }
    }

    // remove outliers in ragged edges
    let loess = LinearLoess::new(loess_width);
    let horizon = loess.smooth(&horizon);

    Ok(horizon)
}

/// this is much faster than canny and normally yields better results
/// TODO - this still needs to be refined for cloudy skies (esp. around horizon line) and for non-vertical foreground (structure) like cables (valid sky below)
/// the latter one can be filtered out by using a vertical lookahead distance below the structure line to find sky pixels
pub fn hsv_horizon_line (img: &DynamicImage, offset: u32, y_dist: u32, v_diff: f32, s_diff: f32, loess_width: usize)->Result<Vec<u32>> {
    let (w,h) = img.dimensions();
    let mut horizon: Vec<u32> = vec![offset; w as usize];
    let mut v_col: Vec<f32> = vec![0.0; h as usize];
    let mut s_col: Vec<f32> = vec![0.0; h as usize];
    let mut img = img.as_rgb8().ok_or( OdinImageError::InvalidImageFormat("not an RGB image".into()))?;
    let y1 = offset + y_dist;

    'outer: for x in 0..w {
        for y in offset..h {
            let [r,g,b] = img.get_pixel( x, y).0;
            let (h,s,v) = rgb_to_hsv(r,g,b);

            v_col[y as usize] = v;
            s_col[y as usize] = s;

            // we could also do a [h,v] test for blue sky - if outside these parameters we don't have to loop
            // h: 210..240
            // v: 0.8..1.0

            //--- gradient test
            let y0 = if y > y1 { y - y_dist } else { offset } as usize;

            // we look for dV < CV or dS > CS conditions
            if (v - (v_col[y0]) < v_diff) || ((s - s_col[y0]) > s_diff) { 
                horizon[x as usize] = y;
                continue 'outer;
            }
        }
    }

    // remove outliers in ragged edges
    let loess = LinearLoess::new(loess_width);
    let horizon = loess.smooth(&horizon);

    Ok( horizon )
}


/// apply offset and constrain horizon values to [min_horizon..max_horizon]
pub fn offset_horizon (horizon: &mut Vec<u32>, offset: i32, min_horizon: u32, max_horizon: u32) {
    let min_horizon = min_horizon as i32;
    let max_horizon = max_horizon as i32;

    for i in 0..horizon.len() {
        let mut y = horizon[i] as i32 + offset;
        if y < min_horizon { 
            y = min_horizon; 
        } else if y > max_horizon { 
            y = max_horizon; 
        }
        horizon[i] = y as u32;
    }
}

pub fn terrain_tile_mask ( img: &DynamicImage, tile_width: u32, tile_height: u32, fractional_tiles: bool, horizon: &[u32])->Result<Mask> {
    let (nx,ny) = get_grid_dim( img, tile_width, tile_height, fractional_tiles);
    let mut mask = Mask::new( nx, ny);
    let mut x: u32 = 0;

    for i in 0..nx {
        let y_horiz = min_horizon( horizon, x as usize, tile_width as usize);
        let mut y: u32 = tile_height; // the lower tile y coordinate

        for j in 0..ny {
            if y > y_horiz {
                mask.set(i,j);
            }
            y += tile_height;
        }
        x += tile_width;
    }

    Ok( mask )
}

fn min_horizon (horizon: &[u32], i0: usize, width: usize)->u32 {
    let mut min_y = u32::MAX;

    for i in i0..(i0+width).min( horizon.len()) {
        if min_y > horizon[i] {
            min_y = horizon[i];
        }
    }

    min_y
}

pub fn save_horizon<P> (horizon: &[u32], path: P)->Result<()> where P: AsRef<Path> {
    let s = serde_json::to_string( horizon)?;
    let mut file = File::create( path)?;
    file.write_all( s.as_bytes())?;
    Ok(())
}

pub fn load_horizon<P> (path: P)->Result<Vec<u32>> where P: AsRef<Path> {
    let mut file = File::open(path)?;
    let mut json = String::with_capacity( file.metadata()?.len() as usize);
    file.read_to_string(&mut json)?;
    let horizon: Vec<u32> = serde_json::from_str(&json)?;
    Ok(horizon)
}

pub fn load_checked_horizon<P> (path: P, width: u32)->Result<Vec<u32>> where P: AsRef<Path> {
    let horizon = load_horizon(path)?;
    if horizon.len() == width as usize { 
        Ok( horizon )
    } else {
        Err( OdinImageError::InvalidDimensions("incompatible horizon data".into()))
    }
}

/// load horizon from file or fall back to computing it from image and given parameters
/// if horizon is loaded from file check against image width
pub fn get_horizon<P> (
    horizon_file: Option<&P>, 
    img: &DynamicImage, 
    top_margin: u32, y_dist: u32, v_diff: f32, s_diff: f32, loess_width: usize
)->Result<Vec<u32>> where P: AsRef<Path> 
{
    if let Some(path) = horizon_file {
        load_checked_horizon( path, img.width())
    } else {
        hsv_horizon_line( img, top_margin, y_dist, v_diff, s_diff, loess_width)
    }
}

/* #endregion edge detection */

/* #region blur/noise reduction **************************************************************************/

pub fn blur (img: &DynamicImage, params: GaussianBlurParameters)->Result<DynamicImage> {
    Ok( img.blur_advanced(params) )
}

/* #endregion blur/noise reduction */

/* #region HSV analysis *******************************************************************************************/

pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let h = if delta == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };

    let h = if h < 0.0 { h + 360.0 } else { h };

    let s = if max == 0.0 { 0.0 } else { delta / max };

    (h, s, max)
}


/// get mean HSV values for given tile, only considering tiles above a threshold ratio of valid (non-filtered) pixels
pub fn mean_hsv<F> (img: &SubImage<&DynamicImage>, min_valid: f32, pred: F) -> Result<(f32,f32,f32)> where F: Fn(u32,u32,&(f32,f32,f32))->bool {
    let (w,h) = img.dimensions();

    let mut h_sum: f64 = 0.0;
    let mut s_sum: f64 = 0.0;
    let mut v_sum: f64 = 0.0;
    let mut n = w*h;

    let (x0,y0) = img.offsets();

    for y in 0..h {
        for x in 0..w {
            let [r, g, b,_] = img.get_pixel(x, y).0;
            let hsv = rgb_to_hsv( r, g, b);

            if pred( x + x0, y + y0, &hsv)  { 
                h_sum += hsv.0 as f64;
                s_sum += hsv.1 as f64;
                v_sum += hsv.2 as f64;
            } else {
                n -= 1;
            }
        }
    }

    if (n as f32 / (w*h) as f32) >= min_valid { 
        let n = n as f64;
        Ok( ( (h_sum / n) as f32, (s_sum / n) as f32, (v_sum / n) as f32 ) )
    } else {
        Err( OdinImageError::InsufficientData("not enough valid tile pixels".into()))
    }
}

pub fn tiled_mean_hsv<F> (
    img: &DynamicImage, tile_width: u32, tile_height: u32, fractional_tiles: bool, mask: Option<&Mask>, 
    min_valid: f32, valid_pixel_pred: &F
) -> Result<(TileData<f32>,TileData<f32>,TileData<f32>)>  
    where F: Fn(u32,u32,&(f32,f32,f32))->bool + Clone // WATCH OUT - the closure should only capture references
{
    let (nx, ny) = get_grid_dim( img, tile_width, tile_height, fractional_tiles);
    let mut h_data: TileData<f32> = TileData::new( nx, ny);
    let mut s_data: TileData<f32> = TileData::new( nx, ny); 
    let mut v_data: TileData<f32> = TileData::new( nx, ny); 

    let mut compute_hsv_tile_data = |sub: &SubImage<&DynamicImage>, p: (usize,usize)| {
        if let Ok( (h,s,v) ) = mean_hsv( sub, min_valid, valid_pixel_pred) {
            h_data.set( p.0, p.1, h);
            s_data.set( p.0, p.1, s);
            v_data.set( p.0, p.1, v);
        }
    };

    process_subimage_tiles( img, tile_width, tile_height, fractional_tiles, mask, compute_hsv_tile_data)?;
    Ok( (h_data,s_data,v_data) )
}


/* #endregion HSV analysis */


/* #region gray/white factors *************************************************************************************/


const WHITE_LEN: f64 = 441.6729559300637; // length of white-vector: (3 * (255^2)).sqrt()
const MAX_A: f64 = 0.9553166181245093; // max angle between [r,b,g] vector and white vector [255,255,255] in radians (~54.7356 deg)


/// computes gray-/white-ness factors for a given RGB value. 
/// The grayness factor [0..1] represents the relative angle between the given [r,g,b] vector and the white [255,255,255] vector. A value
/// of 0 represents the max angle (on either one of the R, G or B axis), 1 means [r,g,b] is colinear with the white vector (the color is perfect gray)
/// The whiteness factor [0..1] represents the relative length of the [r,g,b] vector projection onto the white vector. A value of 0 means black,
/// a value of 1 means white
fn rgb_to_gw (rgb: &[u8;3])->(f32,f32) {
    let [r,g,b,] = *rgb;

    // the two singularities
    if r|g|b ==   0 { return (1.0,0.0) } // black
    if r&g&b == 255 { return (1.0,1.0) } // white

    let r = r as u32;
    let g = g as u32;
    let b = b as u32;
    let len = (((r*r) + (g*g) + (b*b)) as f64).sqrt();
    let dot = (r*255 + g*255 + b*255) as f64;
    let cos_a = dot / (len * WHITE_LEN);
    
    let gray = 1.0 - (cos_a.acos() / MAX_A); // relative grayness: 1 is perfect gray, 0 is pure R,G,B
    let white = (len * cos_a) / WHITE_LEN; // relative whiteness = projection of rgb vector on white-vector (gray axis)

    (gray as f32, white as f32)
}

fn rgba_to_gw (rgba: &[u8;4])->(f32,f32) {
    let [r,g,b,_] = *rgba;

    // the two singularities
    if r|g|b ==   0 { return (1.0,0.0) } // black
    if r&g&b == 255 { return (1.0,1.0) } // white

    let r = r as u32;
    let g = g as u32;
    let b = b as u32;
    let len = (((r*r) + (g*g) + (b*b)) as f64).sqrt();
    let dot = (r*255 + g*255 + b*255) as f64;
    let cos_a = dot / (len * WHITE_LEN);
    
    let gray = 1.0 - (cos_a.acos() / MAX_A); // relative grayness
    let white = (len * cos_a) / WHITE_LEN; // relative whiteness = projection of rgb vector on white-vector (gray axis)

    (gray as f32, white as f32)
}

pub fn mean_gw <F> (img: &SubImage<&DynamicImage>, min_valid: f32, pred: F) -> Result<(f32,f32)> where F: Fn(u32,u32,&(f32,f32))->bool {
    let (w,h) = img.dimensions();
    let (x0,y0) = img.offsets();

    let mut g_sum: f64 = 0.0;
    let mut w_sum: f64 = 0.0;
    let mut n = w*h;

    for y in 0..h {
        for x in 0..w {
            let rgba = img.get_pixel(x, y).0;
            let gw = rgba_to_gw( &rgba);

            if pred( x + x0, y + y0, &gw)  { 
                g_sum += gw.0 as f64;
                w_sum += gw.1 as f64;
            } else {
                n -= 1;
            }
        }
    }

    if (n as f32 / (w*h) as f32) >= min_valid { 
        let n = n as f64;
        Ok( ((g_sum/n) as f32, (w_sum/n) as f32) )
    } else {
        Err( OdinImageError::InsufficientData("not enough valid tile pixels".into()))
    }
}

pub fn tiled_mean_gw<F> (
    img: &DynamicImage, tile_width: u32, tile_height: u32, fractional_tiles: bool, mask: Option<&Mask>,
    min_valid: f32, valid_pixel_pred: &F
) -> Result<(TileData<f32>,TileData<f32>)>  
    where F: Fn(u32,u32,&(f32,f32))->bool //+ Clone // WATCH OUT - the closure should only capture references
{
    let (nx, ny) = get_grid_dim( img, tile_width, tile_height, fractional_tiles);
    let mut gray_data: TileData<f32> = TileData::new( nx, ny);
    let mut white_data: TileData<f32> = TileData::new( nx, ny); 

    let mut compute_gw_tile_data = |sub: &SubImage<&DynamicImage>, p: (usize,usize)| {
        if let Ok( (g,w) ) = mean_gw( sub, min_valid, valid_pixel_pred) {
            gray_data.set( p.0, p.1, g);
            white_data.set( p.0, p.1, w);
        }
    };

    process_subimage_tiles( img, tile_width, tile_height, fractional_tiles, mask, compute_gw_tile_data)?;
    Ok( (gray_data, white_data) )
}

/* #endregion gray/white factors */

/* #region rgb normalization ********************************************************************************************/

pub struct Rgb8Stats {
    pub r: Stats<u8>,
    pub g: Stats<u8>,
    pub b: Stats<u8>,

    r_hist: [usize;256],
    g_hist: [usize;256],
    b_hist: [usize;256],

    pub rel_white: Stats<f32>,
    pub rel_gray: Stats<f32>,
}

impl Rgb8Stats {
    pub fn new()->Self {
        let r = Stats::<u8>::new();
        let g = Stats::<u8>::new();
        let b = Stats::<u8>::new();

        let r_hist = [0;256];
        let g_hist = [0;256];
        let b_hist = [0;256];

        let rel_white = Stats::<f32>::new();
        let rel_gray = Stats::<f32>::new();

        Rgb8Stats{ r, g, b, r_hist, g_hist, b_hist, rel_white, rel_gray }
    }

    pub fn add_rgb (&mut self, pix: &[u8;3]) {
        self.r.add(pix[0]);
        self.g.add(pix[1]);
        self.b.add(pix[2]);

        self.r_hist[ pix[0] as usize] += 1;
        self.g_hist[ pix[1] as usize] += 1;
        self.b_hist[ pix[2] as usize] += 1;

        let (rg,rw) = rgb_to_gw(pix);
        self.rel_gray.add( rg);
        self.rel_white.add( rw);
    }

    pub fn add_rgba (&mut self, pix: &[u8;4]) {
        self.r.add(pix[0]);
        self.g.add(pix[1]);
        self.b.add(pix[2]);

        self.r_hist[ pix[0] as usize] += 1;
        self.g_hist[ pix[1] as usize] += 1;
        self.b_hist[ pix[2] as usize] += 1;

        let (rg,rw) = rgba_to_gw(pix);
        self.rel_gray.add( rg);
        self.rel_white.add( rw);
    }

    pub fn min (&self)->u8 {
        self.r.min.min( self.g.min.min( self.b.min))
    }

    pub fn upper_min (&self)->u8 {
        self.r.min.max( self.g.min.max( self.b.min))
    }

    pub fn max (&self)->u8 {
        self.r.max.max( self.g.max.max( self.b.max))
    }

    pub fn bounds (&self)->(u8,u8) {
        (self.min(), self.max())
    }

    pub fn bright_bounds (&self)->(u8,u8) {
        (self.upper_min(), self.max())
    }

    fn upper_channel_percentile_bounds (&self, cut: f32, hist: &[usize])->u8 {
        let n = self.r.n;
        let n_upper = (n as f32 * cut) as usize; // the number of upper samples to filter
        let mut n_cut = 0; 

        for upper in (0..256).rev() {
            n_cut += hist[upper];
            if n_cut >= n_upper {
                return upper as u8
            }
        }
        0
    }

    fn lower_channel_percentile_bounds (&self, cut: f32, hist: &[usize])->u8 {
        let n = self.r.n;
        let n_lower = (n as f32 * cut) as usize; // the number of upper samples to filter
        let mut n_cut = 0; 

        for lower in 0..256 {
            n_cut += hist[lower];
            if n_cut >= n_lower {
                return lower as u8
            }
        }
        255
    }

    pub fn upper_percentile_bounds (&self, cut: f32)->u8 {
        let r_upper = self.upper_channel_percentile_bounds(cut, &self.r_hist);
        let g_upper = self.upper_channel_percentile_bounds(cut, &self.g_hist);
        let b_upper = self.upper_channel_percentile_bounds(cut, &self.b_hist);

        r_upper.max( g_upper.max( b_upper))
    }

    pub fn lower_percentile_bounds (&self, cut: f32)->u8 {
        let r_lower = self.lower_channel_percentile_bounds(cut, &self.r_hist);
        let g_lower = self.lower_channel_percentile_bounds(cut, &self.g_hist);
        let b_lower = self.lower_channel_percentile_bounds(cut, &self.b_hist);

        r_lower.min( g_lower.min( b_lower))
    }

    pub fn print (&self, lower_cut: f32, upper_cut: f32) {
        println!("RGB stats:");
        println!("  red:   {:3} .. {:3}", self.r.min, self.r.max);
        println!("  green: {:3} .. {:3}", self.g.min, self.g.max);
        println!("  blue:  {:3} .. {:3}", self.b.min, self.b.max);

        println!("  total: {:3} .. {:3}", self.min(), self.max());
        println!("  {:.0} - {:.0} %:  {:3} .. {:3}", 
            lower_cut * 100.0, 
            100.0 - upper_cut * 100.0, 
            self.lower_percentile_bounds(lower_cut), 
            self.upper_percentile_bounds(upper_cut)
        );

        println!("  rel-gray:  {:.2} .. {:.2} : {:.2}", self.rel_gray.min, self.rel_gray.max, self.rel_gray.mean);
        println!("  rel-white: {:.2} .. {:.2} : {:.2}", self.rel_white.min, self.rel_white.max, self.rel_white.mean);
    }
}

/// get the RGB stats for a potentially masked/filtered image
pub fn filtered_rgb_stats<F> (
    img: &DynamicImage, 
    tile_width: u32, tile_height: u32, fractional_tiles: bool, mask: Option<&Mask>,
    valid_pixel_pred: &F
) -> Result<Rgb8Stats>
    where F: Fn(u32,u32,&[u8;4])->bool
{
    let (w,h) = img.dimensions();
    let (nx,ny) = get_grid_dim( img, tile_width, tile_height, fractional_tiles);
    if let Some(mask) = mask {
        mask.check_dimensions(nx,ny)?;
    }

    let mut rgb_stats = Rgb8Stats::new();
    
    for y in 0..h {
        let y_mask = (y / tile_height) as usize;
        for x in 0..w {
            let x_mask = (x / tile_width) as usize;
            if !is_masked( x_mask, y_mask, &mask) { // only scale pixels in tiles that are not masked
                let rgba = img.get_pixel(x, y).0;
                if valid_pixel_pred( x, y, &rgba) {
                    rgb_stats.add_rgba( &rgba);                                                
                }
            }
        }
    }

    Ok( rgb_stats )
}

pub fn min_max_normalize_filtered_rgb_mut<F> (
    img: &mut DynamicImage, 
    tile_width: u32, tile_height: u32, fractional_tiles: bool, mask: Option<&Mask>,
    lower: u8, upper: u8,
    valid_pixel_pred: &F
) -> Result<()>
    where F: Fn(u32,u32,&[u8;4])->bool
{
    let (w,h) = img.dimensions();
    let (nx,ny) = get_grid_dim( img, tile_width, tile_height, fractional_tiles);

    let lower = lower as f32;
    let upper = upper as f32;
    //let range = upper - lower;

    for j in 0..ny {
        let y = j as u32 * tile_height;
        let th = tile_height.min( h - y);
        for i in 0..nx {
            let x = i as u32 * tile_width;
            let tw = tile_width.min( w - x);
            if !is_masked( i,j, &mask) {
                for sy in y..y+th {
                    for sx in x..x+tw {
                        let rgba = img.get_pixel(sx, sy).0;
                        if valid_pixel_pred( sx,sy, &rgba) {
                            let (l,u) = min_max_rgba( rgba);
                            let lower = (l as f32).min( lower);
                            let upper = (u as f32).max( upper);
                            let range = upper - lower;

                            let r = (((rgba[0] as f32 - lower).max(0.0)/range) * 255.0) as u8;
                            let g = (((rgba[1] as f32 - lower).max(0.0)/range) * 255.0) as u8;                    
                            let b = (((rgba[2] as f32 - lower).max(0.0)/range) * 255.0) as u8;                    
                            img.put_pixel(sx, sy, Rgba([r,g,b,255]));
                        }
                    }
                }
            }
        }
    }

    Ok(())
}


/// note: all scaling has to preserve gray-ness (RGB channels have to be scaled with the same factor)
pub fn enhance_whiteness (
    img: &mut RgbImage, 
    tile_width: u32, tile_height: u32, fractional_tiles: bool, 
    mask: Option<&Mask>, horizon: &[u32], 
    lower_w: f32, upper_w: f32, limit_w: f32
) -> Result<()> {
    let (mut w, mut h) = img.dimensions();
    if !fractional_tiles {
        w = w - w % tile_width;
        h = h - h % tile_height;
    }

    for y in 0..h {
        let y_mask = (y / tile_height) as usize;
        for x in 0..w {
            if horizon[x as usize] <= y {     // only scale below horizon since sky is likely to be saturated
                let x_mask = (x / tile_width) as usize;
                if !is_masked( x_mask, y_mask, &mask) { // only scale pixels in tiles that are not masked
                    let mut rgb = img.get_pixel(x, y).0;
                    normalize_rgb( &mut rgb, lower_w, upper_w, limit_w);                    
                    img.put_pixel( x,  y, Rgb(rgb));
                }
            }
        }
    }

    Ok(())
}

pub fn normalize_rgb (rgb: &mut [u8;3], lower_w: f32, upper_w: f32, limit_w: f32) {
    let (g,w) = rgb_to_gw(rgb);
    
    let ww = if w >= upper_w { // clip whiteness outside bounds to limit
        limit_w 
    } else if w <= lower_w {
        0.0
    } else { // expand whiteness within bounds to limit
        ((w - lower_w) / (upper_w - lower_w)) * limit_w
    };

    let r = rgb[0] as u32;
    let g = rgb[1] as u32;
    let b = rgb[2] as u32;

    let rgb_len = ((r*r + g*g + b*b) as f64).sqrt();
    let c: f32 = ww * (WHITE_LEN / rgb_len) as f32;


    rgb[0] = ((rgb[0] as f32) * c) as u8;
    rgb[1] = ((rgb[1] as f32) * c) as u8;                    
    rgb[2] = ((rgb[2] as f32) * c) as u8; 
}

// using SIMD via the argminmax crate is significantly slower due to loop over array slices (it also requires nightly)

#[inline(always)]
pub fn min_max_rgb (rgb: [u8;3])->(u8,u8) {
    (
        rgb[0].min( rgb[1].min( rgb[2])),
        rgb[0].max( rgb[1].max( rgb[2])),
    )
}

#[inline(always)]
pub fn max_rgb (rgb: [u8;3])->u8 {
    rgb[0].max( rgb[1].max( rgb[2]))
}

#[inline(always)]
pub fn min_max_rgba (rgba: [u8;4])->(u8,u8) {
    (
        rgba[0].min( rgba[1].min( rgba[2])),
        rgba[0].max( rgba[1].max( rgba[2])),
    )
}

#[inline(always)]
pub fn max_rgba (rgba: &[u8;4])->u8 {
    rgba[0].max( rgba[1].max( rgba[2]))
}

//--- various pixel filters to be used from valid_pixel_pred

#[inline]
pub fn is_artificial_color (rgba: &[u8;4])->bool {
    (rgba[0] == 0) || (rgba[1] == 0) || (rgba[2] == 0) 
    || (rgba[0] == 255) || (rgba[1] == 255) || (rgba[2] == 255)
}

#[inline]
pub fn is_glare (rgba: &[u8;4], glare_threshold: f32)->bool {
    let r = rgba[0] as f32;
    let g = rgba[1] as f32;
    let b = rgba[2] as f32;
    ((r*r + g*g + b*b) / 195075.0) >= glare_threshold  // 3*255^2
}

/* #endregion rgb normalization */

/* #region image (region) comparison ************************************************************************************/

pub fn tiled_mean_rgb_diff_norm<F> (
    img1: &DynamicImage, img2: &DynamicImage,
    tile_width: u32, tile_height: u32, fractional_tiles: bool, mask: Option<&Mask>,
    min_valid: f32, valid_pixel_pred: &F
) -> Result<TileData<f32>>  
    where F: Fn(u32,u32, &[u8;4], &[u8;4])->bool
{
    check_equal_dimensions(img1, img2)?;
    let (w,h) = img1.dimensions();
    let (nx, ny) = get_grid_dim( img1, tile_width, tile_height, fractional_tiles);
    let mut diff_data: TileData<f32> = TileData::new( nx, ny);

    for j in 0..ny {
        let y = j as u32 * tile_height;
        let th = tile_height.min( h - y);
        for i in 0..nx {
            let x = i as u32 * tile_width;
            let tw = tile_width.min( w - x);
            if !is_masked( i,j, &mask) {
                let sub1 = img1.view( x, y, tw, th);
                let sub2 = img2.view( x, y, tw, th);

                if let Ok(diff) = mean_rgb_diff_norm( &sub1, &sub2, min_valid, valid_pixel_pred) {
                    diff_data.set( i, j, diff);
                }
            }
        }
    }

    Ok( diff_data )
}

pub fn mean_rgb_diff_norm<F> (sub1: &SubImage<&DynamicImage>, sub2: &SubImage<&DynamicImage>, min_valid: f32, valid_pixel_pred: F)->Result<f32> 
    where F: Fn(u32,u32, &[u8;4], &[u8;4])->bool
{
    let (w,h) = sub1.dimensions();
    if (w,h) != sub2.dimensions() { return Err( OdinImageError::InvalidDimensions("sub image dimensions differ".into())) }
    let (x0,y0) = sub1.offsets();
    // TODO should we check here if offsets differ?

    let mut mean: f64 = 0.0;
    let mut prev_mean = 0.0;
    let mut n: usize = 0;

    for y in 0..h {
        for x in 0..w {
            let rgb1 = sub1.get_pixel(x, y).0;
            let rgb2 = sub2.get_pixel(x, y).0;

            if valid_pixel_pred( x0 + x, y0 + y, &rgb1, &rgb2) {
                let d = rgb_diff_norm( &rgb1, &rgb2);

                prev_mean = mean;
                let nf = n as f64;
                mean = (d + (nf * prev_mean) - prev_mean) / nf;
                n += 1;
            }
        }
    }

    if (n as f32 / (w*h) as f32) >= min_valid { 
        Ok( mean as f32 )
    } else {
        Err( OdinImageError::InsufficientData("not enough valid pixels".into()))
    }
}

/// the mean norm of the difference vector of two rgb vectors (alpha is ignored)
#[inline]
pub fn rgb_diff_norm (rgba1: &[u8;4], rgba2: &[u8;4])->f64 {
    let [r1,g1,b1,_] = *rgba1;
    let [r2,g2,b2,_] = *rgba2;

    let dr = r1 as f64 - r2 as f64;
    let dg = g1 as f64 - g2 as f64;
    let db = b1 as f64 - b2 as f64;

    sqrt( dr*dr + dg*dg + db*db)
}

/* #endregion image comparison */