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

use std::{fs::File, io::Write, path::{Path,PathBuf}, iter::IntoIterator};
use bit_set::BitSet;
use image::{DynamicImage, GenericImageView, GrayImage, Luma};
use serde::{Serialize,Deserialize};
use serde_json;
use odin_common::fs::filepath_contents_as_string;
use crate::{errors::Result, OdinImageError, get_grid_dim};

#[derive(Serialize,Deserialize)]
pub struct Mask {
    width: usize,
    height: usize,
    data: BitSet
}

impl Mask {
    pub fn new (width: usize, height: usize)->Self {
        let data = BitSet::with_capacity(width*height);
        Mask{width,height,data}
    }

    pub fn open<P> (path: P)->Result<Self> where P: AsRef<Path> {
        let file_contents =  filepath_contents_as_string(&path)?;
        Ok( serde_json::from_str( &file_contents.as_str())? )
    }

    pub fn check_dimensions (&self, nx: usize, ny: usize)->Result<()> {
        if self.width == nx && self.height == ny {
            Ok(())
        } else {
            Err( crate::OdinImageError::IncompatibleMask("wrong mask dimensions".into()))
        }
    }

    pub fn open_checked<P> (path: P, width: usize, height: usize)->Result<Self> where P: AsRef<Path> {
        let mask = Self::open(path)?;
        if mask.width == width && mask.height == height {
            Ok(mask)
        } else {
            Err( crate::OdinImageError::IncompatibleMask("wrong dimensions".into()))
        }
    }

    pub fn maybe_open<P> (opt_path: Option<P>)->Result<Option<Self>> where P: AsRef<Path> {
        match opt_path {
            Some(p) => Ok(  Some(Self::open( p)?) ),
            None => Ok( None )
        }
    }

    pub fn maybe_open_checked<P> (opt_path: Option<P>, width: usize, height: usize)->Result<Option<Self>> where P: AsRef<Path> {
        match opt_path {
            Some(p) => Ok(  Some(Self::open_checked( p, width, height)?) ),
            None => Ok( None )
        }
    }

    pub fn open_luma8_image<P> (path: P)->Result<Self> where P: AsRef<Path> {
        let img = image::open(path)?;
        let gray_img = img.as_luma8().ok_or( OdinImageError::InvalidImageFormat("not a luma8 image".into()))?;
        Self::from_luma8_image( &gray_img)
    }

    pub fn from_luma8_image (img: &GrayImage)->Result<Self> {
        let (w,h) = img.dimensions();

        let mut mask = Mask::new( w as usize, h as usize);
        for y in 0..h {
            for x in 0..w {
                if img.get_pixel(x, y).0[0] > 0 {
                    mask.set( x as usize, y as usize);
                }
            }
        }

        Ok( mask )
    }

    pub fn save_as_luma8_image<P> (&self, path: P)->Result<()> where P: AsRef<Path> {
        let mut img = GrayImage::new( self.width as u32, self.height as u32);
        for y in 0..self.height {
            for x in 0..self.width {
                if self.get( x, y) {
                    img.put_pixel(x as u32, y as u32, Luma([255u8]));
                }
            }
        }
        Ok( img.save( path)? )
    }

    pub fn save<P> (&self, path: P)->Result<()> where P: AsRef<Path> {
        let mut file = File::create(path)?;
        let json = serde_json::to_string( self)?;
        Ok( file.write_all( json.as_bytes())? )
    }

    pub fn checked_dimensions (&self, img: &DynamicImage, tile_width: u32, tile_height: u32, fractional_tiles: bool)->Result<(usize,usize)> {
        let (w,h) = self.dimensions();
        let (nx,ny) = get_grid_dim( &img, tile_width, tile_height, fractional_tiles);
        if (w == nx) && (h == ny) {
            Ok( (nx,ny) )
        } else {
            Err( OdinImageError::InvalidDimensions("mask not compatible with image and tile size".into()))
        }
    }

    pub fn dimensions (&self)->(usize,usize) {
        (self.width,self.height)
    }

    pub fn get (&self, x: usize, y: usize)->bool {
        self.data.contains( y*self.width + x)
    }

    pub fn set (&mut self, x: usize, y: usize) {
        self.data.insert( y*self.width + x);
    }

    pub fn unset (&mut self, x: usize, y: usize)->bool {
        self.data.remove( y*self.width + x)
    }

    pub fn clear (&mut self) {
        self.data.clear();
    }

    pub fn print (&self) {
        let (w,h) = self.dimensions();

        print!( "     ");
        for x in 0..w { print!( "{:3}", x); }
        println!();
        print!("    ┌");
        for x in 0..w { print!( "───"); }
        println!("─┐");

        for y in 0..h {
            print!( "{:3} │", y);
            for x in 0..w {
                if self.get(x, y) {
                    print!( "  ◼︎");
                } else {
                    print!("   ");
                }
            }
            println!(" │ {:3}", y);
        }

        print!("    └");
        for x in 0..w { print!( "───"); }
        println!("─┘");
        print!("     ");
        for x in 0..w { print!( "{:3}", x); }
        println!();
    }

    pub fn union (&self, other: &Mask)->Result<Self> {
        if self.dimensions() == other.dimensions() {
            let width = self.width;
            let height = self.height;
            let mut data = self.data.clone();
            data.union_with( &other.data);
            Ok( Mask{width,height,data} )

        } else {
            Err( crate::OdinImageError::IncompatibleMask("masks have different dimensions".into()))
        }
    }

    pub fn intersection (&self, other: &Mask)->Result<Self> {
        if self.dimensions() == other.dimensions() {
            let width = self.width;
            let height = self.height;
            let mut data = self.data.clone();
            data.intersect_with( &other.data);
            Ok( Mask{width,height,data} )

        } else {
            Err( crate::OdinImageError::IncompatibleMask("masks have different dimensions".into()))
        }
    }    

    pub fn iter (&self)->MaskIter<'_> {
        MaskIter{ data: &self.data, w: self.width, n: 0 }
    }
}

impl<'a> IntoIterator for &'a Mask {
    type Item = (usize,usize);
    type IntoIter = MaskIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct MaskIter<'a> {
    data: &'a BitSet,
    w: usize,
    n: usize
}

impl <'a> Iterator for MaskIter<'a> {
    type Item = (usize,usize);
    
    fn next(&mut self) -> Option<Self::Item> {
        while self.n < self.data.len() {
            if self.data.contains( self.n) {
                let x = self.n % self.w;
                let y = self.n / self.w;
                self.n += 1;
                return Some( (x,y) )
            } else {
                self.n += 1;
            }
        }
        None
    }
}