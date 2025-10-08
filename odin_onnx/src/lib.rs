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

use std::{path::{Path,PathBuf}};
use image::{
    DynamicImage, GenericImage, GenericImageView, ImageBuffer, ImageFormat, Rgb, RgbImage,
    imageops::{FilterType,resize},
};

use ndarray::{Array4, Axis, s};
use ort::{
	inputs,
	session::{Session, SessionOutputs, Input, Output},
	value::TensorRef,
};
use serde::{Serialize,Deserialize};
use ron;
use odin_common::fs::EnvPathBuf;

mod errors;
use errors::Result;

/// the policy of how to fit aribtrarily sized input images to (fixed) model size
#[derive(Deserialize,Debug)]
pub enum FitPolicy {
	Scale, // indiscriminately scale test image to model coordinates without preserving aspect ratio
	Pad, // pad scaled test image to model size while preserving aspect ratio
	Mosaic // break up test image into overlapping sub-images which can be scaled to model size while preserving aspect ratio
}

/// the configuration data that specifies the model and target dimensions to use 
#[derive(Deserialize,Debug)]
pub struct ImageClassifierConfig {
    pub name: String, // the model name
    pub model_path: EnvPathBuf,
    pub model_width: u32,
    pub model_height: u32,

	pub fit_policy: FitPolicy,
    pub pad_rgb: [u8; 3],
	pub mosaic_factor: f64
}

/// indiscriminately scale the input image to model size. This does **not** preserve aspect ratio
pub fn fit_scaled (img: &RgbImage, model_w: u32, model_h: u32) -> Result<RgbImage> {
    Ok( resize( img, model_w, model_h, FilterType::CatmullRom))
}

/// scale input image to model input size while preserving aspect ratio, using a pad_rgb color for the padded (blank) part
pub fn fit_padded (img: &RgbImage, pad_rgb: Rgb<u8>, model_w: u32, model_h: u32) -> Result<RgbImage> {
    let w = img.width();
    let h = img.height();

    if w <= model_w && h <= model_h {
        // no scaling
        if w == model_w && h == model_h {
            // no padding either
            Ok(img.clone()) // suboptimal that we have to clone but we don't want to own the input image here
        } else {
            let mut pad_img = RgbImage::from_pixel(model_w, model_h, pad_rgb);
            pad_img.copy_from( img, 0, 0)?;
            Ok(pad_img)
        }
    } else {
        // img has to be scaled down
        // get dominant scale factor
        let sx = model_w as f64 / w as f64;
        let sy = model_h as f64 / h as f64;
        let s = f64::min(sx, sy);

        let ws = (w as f64 * s) as u32;
        let hs = (h as f64 * s) as u32;
        let scaled_img = resize( img, ws, hs, FilterType::CatmullRom); // scale so that it fits into model size

        let mut pad_img = RgbImage::from_pixel(model_w, model_h, pad_rgb);
        pad_img.copy_from(&scaled_img, 0, 0)?;
        Ok(pad_img)
    }
}

/// break up img into overlapping sub-images that can be scaled to model input size with constant aspect ratio (avoiding distortion)
/// overlap is the required overlap factor with respect to output image width [0..1]
pub fn fit_mosaic (img: &RgbImage, overlap: f64, model_w: u32, model_h: u32) -> Result<Vec<RgbImage>> {
	todo!()
}

pub fn fit (img: &RgbImage, config: &ImageClassifierConfig)->Result<Vec<RgbImage>> {
    match config.fit_policy {
        FitPolicy::Scale => fit_scaled( img, config.model_width, config.model_height).map( |img| vec![img]),
        FitPolicy::Pad => fit_padded( img, config.pad_rgb.into(), config.model_width, config.model_height).map( |img| vec![img]),
        FitPolicy::Mosaic => fit_mosaic( img, config.mosaic_factor, config.model_width, config.model_height)
    }
}

pub fn get_inference_input (img: &RgbImage)->Result<Array4<f32>> {
	//let mut input = Array3::zeros( (3, img.width() as usize, img.height() as usize));
	let mut input = Array4::zeros((1, 3, img.width() as usize, img.height() as usize)); // TODO - this should be a 3dim model

	for (x, y, rgb) in img.enumerate_pixels() {
		input[[0, 0, y as usize, x as usize]] = rgb.0[0] as f32 / 255.0;
		input[[0, 1, y as usize, x as usize]] = rgb.0[1] as f32 / 255.0;
		input[[0, 2, y as usize, x as usize]] = rgb.0[2] as f32 / 255.0;
	}

    Ok(input)
}

pub fn run_inference<'a> (session: &'a mut Session, config: &ImageClassifierConfig, img: &'a RgbImage) -> Result<SessionOutputs<'a>>  {
    get_inference_input(img).and_then( |input|{
        Ok( session.run( inputs![ "images" => TensorRef::from_array_view( &input)?])? )
    })
}

pub fn print_session_info (session: &Session)->Result<()> {
    let meta = session.metadata()?;

    println!("model name:   {}", meta.name()?);
    println!("model domain: {}", meta.domain()?);

    println!("model inputs:");
    for input in &session.inputs {
        println!("  input name: {}", input.name);
        println!("  input type: {:?}", input.input_type);
    }

    println!("model outputs:");
    for output in &session.outputs {
        println!("  output name: {}", output.name);
        println!("  output type: {:?}", output.output_type);
    }

    Ok(())
}