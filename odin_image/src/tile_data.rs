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

use std::{fs::File, io::{BufRead, BufReader, Seek, Write}, ops::{Add, Div, Mul, Sub}, path::Path};
use image::{codecs::tiff::TiffDecoder, ImageDecoder};
use ndarray::Array2;
use num::{Zero,Bounded};
use tiff::{
    decoder::{Decoder,DecodingResult},
    encoder::{
        Compression as TiffCompression, DeflateLevel, TiffEncoder, TiffValue,
        colortype::{ColorType,Gray32Float,Gray64Float,Gray8,GrayI8,Gray16,GrayI16,Gray32,GrayI32,Gray64,GrayI64}
    }
};
use odin_common::fs::{extension};
use crate::{ Stats, errors::{Result,OdinImageError}};

pub enum SearchDir {
    RowMajor, ColMajor
}

/// this represents a 2D matrix that is used as a container for computed tile data
pub struct TileData<T> {
    width: usize,
    height: usize,
    data: Vec<T>
}

impl<T> TileData<T> 
    where T: Add<T,Output=T> + Sub<T,Output=T> + Div<T,Output=T> + Mul<T,Output=T> + 
             Bounded + PartialOrd + PartialEq + Zero + Into<f64> + Copy + TiffDataType 
{

    pub fn new (width: usize, height: usize)->Self {  
        let data = vec![ T::zero(); width*height ];
        TileData{width,height,data}
    }

    pub fn open<P> (path: P)->Result<Self> where P: AsRef<Path> {
        Self::check_path( &path)?;
        let mut file: File = File::open(path)?;
        let reader = BufReader::new(file);
        let mut decoder = Decoder::new( reader)?;

        let (w, h) = decoder.dimensions()?;
        let width = w as usize;
        let height = h as usize;
        let result = decoder.read_image()?;
        
        let data = T::get_data( result)?;

        Ok( TileData{width,height,data} )
    }

    pub fn save<P> (&self, path: P)->Result<()> where P: AsRef<Path> {
        Self::check_path( &path)?;

        let mut out_file: File = File::create_new( path.as_ref())?;
        let mut enc = TiffEncoder::new(&mut out_file)?.with_compression( TiffCompression::Deflate(DeflateLevel::Best));

        Ok( T::write_image( &mut enc, self.width, self.height, self.data.as_ref())? )
    }

    fn check_path<P> (path: &P)->Result<()> where P: AsRef<Path> {
        let ext = extension( path);
        if ext.is_none() || !ext.unwrap().ends_with("tif") {
            Err( OdinImageError::IllegalArgument(format!("map only supports TIFF as external format")))
        } else {
            Ok(())
        }
    }

    pub fn dimensions(&self)->(usize,usize) {
        (self.width, self.height)
    }

    pub fn width(&self)->usize { self.width }
    pub fn height(&self)->usize { self.height }
    pub fn len(&self)->usize { self.data.len() }

    #[inline(always)]
    pub fn get(&self, x:usize, y: usize)->T { self.data[ y*self.width + x] }

    #[inline(always)]
    pub fn set(&mut self, x: usize, y: usize, v: T) { self.data[ y*self.width + x] = v; }

    pub fn stats(&self)->Stats<T> {
        let mut stats: Stats<T> = Stats::new();
        for i in 0..self.data.len() {
            stats.add( self.data[i]);
        }
        stats
    }

    pub fn print (&self, size: usize, decimals: usize) {
        let (w,h) = self.dimensions();

        print!( "     ");
        for x in 0..w { print!( "{:size$}", x); }
        println!();
        print!("    ┌");
        for x in 0..w*size { print!( "─"); }
        println!("┐");

        for y in 0..h {
            print!( "{:3} │", y);
            for x in 0..w {
                let v: f64 = self.get(x, y).into();
                if !v.is_nan() && v != 0.0 {
                    print!( "{:size$.decimals$}", v);
                } else {
                    print!("{:size$}", "");
                }
            }
            println!("│ {:3}", y);
        }

        print!("    └");
        for x in 0..w*size { print!( "─"); }
        println!("┘");
        print!("     ");
        for x in 0..w { print!( "{:size$}", x); }
        println!();
    }

    pub fn abs_diff (&self, other: &TileData<T>)->Result<Self> {
        if self.dimensions() != other.dimensions() { return Err( OdinImageError::OpFailed("tile data dimensions differ".into())) }
        let n = self.data.len();
        let mut data = Vec::with_capacity(n);

        for i in 0..n {
            let a = self.data[i];
            let b = other.data[i];
            let abs_diff = if a > b { a - b } else { b - a };
            data.push( abs_diff)
        }

        let (width,height) = self.dimensions();
        Ok( TileData{width,height,data} )
    }

    pub fn diff (&self, other: &TileData<T>)->Result<Self> {
        if self.dimensions() != other.dimensions() { return Err( OdinImageError::OpFailed("tile data dimensions differ".into())) }
        let n = self.data.len();
        let mut data = Vec::with_capacity(n);

        for i in 0..n {
            let a = self.data[i];
            let b = other.data[i];
            let diff = a - b;
            data.push( diff)
        }

        let (width,height) = self.dimensions();
        Ok( TileData{width,height,data} )
    }

    pub fn rel_diff (&self, other: &TileData<T>)->Result<TileData<f32>> {
        if self.dimensions() != other.dimensions() { return Err( OdinImageError::OpFailed("tile data dimensions differ".into())) }
        let n = self.data.len();
        let mut data = Vec::with_capacity(n);

        for i in 0..n {
            let a: f64 = self.data[i].into();
            let b: f64 = other.data[i].into();
            let rel_diff = (a - b) / b;
            data.push( rel_diff as f32)
        }

        let (width,height) = self.dimensions();
        Ok( TileData{width,height,data} )
    }

    pub fn sum (&self, other: &TileData<T>)->Result<Self> {
        if self.dimensions() != other.dimensions() { return Err( OdinImageError::OpFailed("tile data dimensions differ".into())) }
        let n = self.data.len();
        let mut data = Vec::with_capacity(n);

        for i in 0..n {
            let a = self.data[i];
            let b = other.data[i];
            let sum = a + b;
            data.push( sum)
        }

        let (width,height) = self.dimensions();
        Ok( TileData{width,height,data} )
    }

    pub fn scalar_mul (&self, factor: T)->Self {  // TODO do we need a Result?
        let n = self.data.len();
        let mut data = Vec::with_capacity(n);

        for i in 0..n {
            let prod = self.data[i] * factor;
            data.push( prod)
        }

        let (width,height) = self.dimensions();
        TileData{width,height,data}
    }

    pub fn scalar_div (&self, divisor: T)->Self {  // TODO do we need a Result?
        let n = self.data.len();
        let mut data = Vec::with_capacity(n);

        for i in 0..n {
            let quot = self.data[i] / divisor;
            data.push( quot)
        }

        let (width,height) = self.dimensions();
        TileData{width,height,data}
    }

    pub fn matching<F> (&self, pred: F)->Vec<(usize,usize)> where F: Fn(usize,usize)->bool {
        let mut cells = Vec::new();
        for y in 0..self.height {
            for x in 0..self.width {
                if pred( x,y) {
                    cells.push( (x, y))
                }
            }
        }
        cells
    }


    pub fn matching_row_major<F> (&self, pred: F)->Vec<(usize,usize)> where F: Fn(T)->bool {
        let mut cells = Vec::new();
        for y in 0..self.height {
            for x in 0..self.width {
                if pred( self.get(x,y)) {
                    cells.push( (x, y))
                }
            }
        }
        cells
    }

    pub fn matching_col_major<F> (&self, pred: F)->Vec<(usize,usize)> where F: Fn(T)->bool {
        let mut cells = Vec::new();
        for x in 0..self.width {
            for y in 0..self.height {
                if pred( self.get(x,y)) {
                    cells.push( (x, y))
                }
            }
        }
        cells
    }

    pub fn greater_equal_cells (&self, threshold: T, search_dir: SearchDir)->Vec<(usize,usize)> {
        match search_dir {
            SearchDir::RowMajor => self.matching_row_major( |v| v >= threshold),
            SearchDir::ColMajor => self.matching_col_major( |v| v >= threshold)
        }
    }

    pub fn less_equal_cells (&self, threshold: T, search_dir: SearchDir)->Vec<(usize,usize)> {
        match search_dir {
            SearchDir::RowMajor => self.matching_row_major( |v| v <= threshold),
            SearchDir::ColMajor => self.matching_col_major( |v| v <= threshold)
        }
    }

    //... and more operators to follow 
}

impl<T> Sub for TileData<T>
    where T: Add<T,Output=T> + Sub<T,Output=T> + Div<T,Output=T> + Mul<T,Output=T> + 
             Bounded + PartialOrd + PartialEq + Zero + Into<f64> + Copy + TiffDataType 
{
    type Output = TileData<T>;
    fn sub(self, rhs: Self) -> Self::Output { self.diff( &rhs).unwrap() }
}

impl<T> Add for TileData<T>
    where T: Add<T,Output=T> + Sub<T,Output=T> + Div<T,Output=T> + Mul<T,Output=T> + 
             Bounded + PartialOrd + PartialEq + Zero + Into<f64> + Copy + TiffDataType 
{
    type Output = TileData<T>;
    fn add(self, rhs: Self) -> Self::Output { self.sum( &rhs).unwrap() }
}


pub trait TiffDataType where Self: Sized {
    fn write_image<W> (enc: &mut TiffEncoder<W>, w: usize, h: usize, data: &[Self])->Result<()> where W: Write + Seek;
    fn get_data (result: DecodingResult)->Result<Vec<Self>>;
}

impl TiffDataType for f32 {
    fn write_image<W> (enc: &mut TiffEncoder<W>, w: usize, h: usize, data: &[Self])->Result<()> where W: Write + Seek {
        Ok( enc.write_image::<Gray32Float>( w as u32, h as u32, data)? )
    }

    fn get_data (result: DecodingResult)->Result<Vec<Self>> {
        match result {
            DecodingResult::F32(data) => Ok( data ),
            DecodingResult::F64(data) => Ok( data.iter().map(|&x| x as Self).collect() ),
            _ => Err( OdinImageError::InvalidImageFormat("TIFF does not contain data that can be converted to f32".into()) )
        }
    }
}

impl TiffDataType for f64 {
    fn write_image<W> (enc: &mut TiffEncoder<W>, w: usize, h: usize, data: &[Self])->Result<()> where W: Write + Seek {
        Ok( enc.write_image::<Gray64Float>( w as u32, h as u32, data)? )
    }

    fn get_data (result: DecodingResult)->Result<Vec<Self>> {
        match result {
            DecodingResult::F64(data) => Ok( data ),
            DecodingResult::F32(data) => Ok( data.iter().map(|&x| x as Self).collect() ),
            _ => Err( OdinImageError::InvalidImageFormat("TIFF does not contain data that can be converted to f64".into()) )
        }
    }
}

impl TiffDataType for u8 {
    fn write_image<W> (enc: &mut TiffEncoder<W>, w: usize, h: usize, data: &[Self])->Result<()> where W: Write + Seek {
        Ok( enc.write_image::<Gray8>( w as u32, h as u32, data)? )
    }

    fn get_data (result: DecodingResult)->Result<Vec<Self>> {
        match result {
            DecodingResult::U8(data) => Ok( data ),
            _ => Err( OdinImageError::InvalidImageFormat("TIFF does not contain data that can be converted to u8".into()) )
        }
    }
}

impl TiffDataType for i8 {
    fn write_image<W> (enc: &mut TiffEncoder<W>, w: usize, h: usize, data: &[Self])->Result<()> where W: Write + Seek {
        Ok( enc.write_image::<GrayI8>( w as u32, h as u32, data)? )
    }

    fn get_data (result: DecodingResult)->Result<Vec<Self>> {
        match result {
            DecodingResult::I8(data) => Ok( data ),
            _ => Err( OdinImageError::InvalidImageFormat("TIFF does not contain data that can be converted to i8".into()) )
        }
    }
}

impl TiffDataType for u16 {
    fn write_image<W> (enc: &mut TiffEncoder<W>, w: usize, h: usize, data: &[Self])->Result<()> where W: Write + Seek {
        Ok( enc.write_image::<Gray16>( w as u32, h as u32, data)? )
    }

    fn get_data (result: DecodingResult)->Result<Vec<Self>> {
        match result {
            DecodingResult::U16(data) => Ok( data ),
            DecodingResult::U8(data) => Ok( data.iter().map(|&x| x as Self).collect() ),
            _ => Err( OdinImageError::InvalidImageFormat("TIFF does not contain data that can be converted to u16".into()) )
        }
    }
}

impl TiffDataType for i16 {
    fn write_image<W> (enc: &mut TiffEncoder<W>, w: usize, h: usize, data: &[Self])->Result<()> where W: Write + Seek {
        Ok( enc.write_image::<GrayI16>( w as u32, h as u32, data)? )
    }

    fn get_data (result: DecodingResult)->Result<Vec<Self>> {
        match result {
            DecodingResult::I16(data) => Ok( data ),
            DecodingResult::I8(data) => Ok( data.iter().map(|&x| x as Self).collect() ),
            _ => Err( OdinImageError::InvalidImageFormat("TIFF does not contain data that can be converted to i16".into()) )
        }
    }
}

impl TiffDataType for u32 {
    fn write_image<W> (enc: &mut TiffEncoder<W>, w: usize, h: usize, data: &[Self])->Result<()> where W: Write + Seek {
        Ok( enc.write_image::<Gray32>( w as u32, h as u32, data)? )
    }

    fn get_data (result: DecodingResult)->Result<Vec<Self>> {
        match result {
            DecodingResult::U32(data) => Ok( data ),
            DecodingResult::U16(data) => Ok( data.iter().map(|&x| x as Self).collect() ),
            DecodingResult::U8(data) => Ok( data.iter().map(|&x| x as Self).collect() ),
            _ => Err( OdinImageError::InvalidImageFormat("TIFF does not contain data that can be converted to u32".into()) )
        }
    }
}

impl TiffDataType for i32 {
    fn write_image<W> (enc: &mut TiffEncoder<W>, w: usize, h: usize, data: &[Self])->Result<()> where W: Write + Seek {
        Ok( enc.write_image::<GrayI32>( w as u32, h as u32, data)? )
    }

    fn get_data (result: DecodingResult)->Result<Vec<Self>> {
        match result {
            DecodingResult::I32(data) => Ok( data ),
            DecodingResult::I16(data) => Ok( data.iter().map(|&x| x as Self).collect() ),
            DecodingResult::I8(data) => Ok( data.iter().map(|&x| x as Self).collect() ),
            _ => Err( OdinImageError::InvalidImageFormat("TIFF does not contain data that can be converted to i32".into()) )
        }
    }
}

impl TiffDataType for u64 {
    fn write_image<W> (enc: &mut TiffEncoder<W>, w: usize, h: usize, data: &[Self])->Result<()> where W: Write + Seek {
        Ok( enc.write_image::<Gray64>( w as u32, h as u32, data)? )
    }

    fn get_data (result: DecodingResult)->Result<Vec<Self>> {
        match result {
            DecodingResult::U64(data) => Ok( data ),
            DecodingResult::U32(data) => Ok( data.iter().map(|&x| x as Self).collect() ),
            DecodingResult::U16(data) => Ok( data.iter().map(|&x| x as Self).collect() ),
            DecodingResult::U8(data) => Ok( data.iter().map(|&x| x as Self).collect() ),
            _ => Err( OdinImageError::InvalidImageFormat("TIFF does not contain data that can be converted to u64".into()) )
        }
    }
}

impl TiffDataType for i64 {
    fn write_image<W> (enc: &mut TiffEncoder<W>, w: usize, h: usize, data: &[Self])->Result<()> where W: Write + Seek {
        Ok( enc.write_image::<GrayI64>( w as u32, h as u32, data)? )
    }

    fn get_data (result: DecodingResult)->Result<Vec<Self>> {
        match result {
            DecodingResult::I64(data) => Ok( data ),
            DecodingResult::I32(data) => Ok( data.iter().map(|&x| x as Self).collect() ),
            DecodingResult::I16(data) => Ok( data.iter().map(|&x| x as Self).collect() ),
            DecodingResult::I8(data) => Ok( data.iter().map(|&x| x as Self).collect() ),
            _ => Err( OdinImageError::InvalidImageFormat("TIFF does not contain data that can be converted to i64".into()) )
        }
    }
}