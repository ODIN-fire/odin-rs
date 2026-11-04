/*
 * Copyright 2026 Peter Mehlitz
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://apache.org
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! text recognition (OCR) in images, based on the [ocr-rs](https://crates.io/crates/ocr-rs) crate (PP-OCR detection
//! and recognition models with MNN inference). The models have to be in `$ODIN_ROOT/data/odin_image/ocr`.
//! This module is only available with the "ocr" feature.
//!
//! Note that the [OcrEngine] can be shared between threads, but recognition is CPU intensive and blocking, i.e. it should
//! be called from a blocking context (e.g. `spawn_blocking`) in async code

use std::path::{Path, PathBuf};
use image::DynamicImage;
use odin_build::pkg_data_dir;

pub use ocr_rs::OcrEngine;
use crate::errors::{OdinImageError, Result};

/// model file names (PP-OCRv6 small)
pub const OCR_DET_MODEL: &str = "PP-OCRv6_small_det.mnn";
pub const OCR_REC_MODEL: &str = "PP-OCRv6_small_rec.mnn";
pub const OCR_CHARSET: &str = "ppocr_keys_v6_small.txt";

/// a recognized text with its confidence [0..1] and the upper left corner of its bounding box (in pixels of the image
/// that was passed into the recognition function)
#[derive(Debug, Clone)]
pub struct OcrText {
    pub text: String,
    pub confidence: f32,
    pub left: i32,
    pub top: i32,
}

/// the default directory with the OCR models
pub fn default_ocr_model_dir ()->PathBuf {
    pkg_data_dir!().join("ocr")
}

/// create an OCR engine with the models in the given directory
pub fn create_ocr_engine (model_dir: impl AsRef<Path>)->Result<OcrEngine> {
    let dir = model_dir.as_ref();
    OcrEngine::new( dir.join( OCR_DET_MODEL), dir.join( OCR_REC_MODEL), dir.join( OCR_CHARSET), None)
        .map_err( |e| OdinImageError::OcrError( format!("failed to create OCR engine from {dir:?}: {e}")))
}

/// create an OCR engine with the models in the default directory ($ODIN_ROOT/data/odin_image/ocr)
pub fn create_default_ocr_engine ()->Result<OcrEngine> {
    create_ocr_engine( default_ocr_model_dir())
}

/// recognize all texts in the image
pub fn recognize_text (engine: &OcrEngine, img: &DynamicImage)->Result<Vec<OcrText>> {
    let results = engine.recognize( img).map_err( |e| OdinImageError::OcrError( e.to_string()))?;
    Ok( results.into_iter().map( |r| OcrText {
        text: r.text,
        confidence: r.confidence,
        left: r.bbox.rect.left(),
        top: r.bbox.rect.top(),
    }).collect())
}

/// recognize texts within the given image region (which is clipped to the image bounds). Text positions are relative
/// to the region. Restricting recognition to a known region is considerably faster and more reliable
pub fn recognize_text_in_region (engine: &OcrEngine, img: &DynamicImage, x: u32, y: u32, width: u32, height: u32)->Result<Vec<OcrText>> {
    if x >= img.width() || y >= img.height() {
        return Err( OdinImageError::InvalidRegion( format!("region origin ({x},{y}) outside of image")))
    }
    let w = width.min( img.width() - x);
    let h = height.min( img.height() - y);
    recognize_text( engine, &img.crop_imm( x, y, w, h))
}

/// recognize texts in the bottom `height` pixels of the image file at `path` (e.g. for overlay text lines)
pub fn recognize_bottom_text_in_file (engine: &OcrEngine, path: impl AsRef<Path>, height: u32)->Result<Vec<OcrText>> {
    let img = image::open( path)?;
    let h = height.min( img.height());
    recognize_text_in_region( engine, &img, 0, img.height() - h, img.width(), h)
}
