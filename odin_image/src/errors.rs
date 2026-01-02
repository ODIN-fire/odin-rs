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

use thiserror::Error;
use odin_common::define_error;

pub type Result<T> = std::result::Result<T, OdinImageError>;

define_error!{ pub OdinImageError = 
    ImageError(#[from] image::ImageError) : "Image error {0}",
    IOError(#[from] std::io::Error) : "IO error: {0}",
    TiffError(#[from] tiff::TiffError) : "Tiff error: {0}",
    SerdeError(#[from]serde_json::Error) : "serialization/deserialization error: {0}",
    InvalidImageFormat(String) : "invalid image format: {0}",
    IncompatibleMask(String) : "incompatible mask: {0}",
    InvalidFont( #[from] ab_glyph::InvalidFont) : "Invalid font error: {0}",
    InvalidDimensions(String) : "invalid dimension error: {0}",
    InvalidRegion(String) : "invalid region error: {0}",
    CompareError(#[from] image_compare::CompareError) : "Compare error: {0}",
    NotFoundError(String) : "not found: {0}",
    IllegalArgument(String) : "illegal argument: {0}",
    InsufficientData (String) : "insufficient data: {0}",
    OpFailed(String) : "operation failed: {0}"
}