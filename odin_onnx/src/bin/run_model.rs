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

use std::path::PathBuf;
use anyhow::{Result, anyhow};
use image;

use ndarray::{Array, Array4, Axis, s};
use ort::{
        session::{Session, SessionOutputs},
        value::{Tensor,TensorRef,ValueRef,TensorValueType}
};

use odin_common::{define_cli, ron};
use odin_onnx::{
    fit, print_session_info, img_to_array4, ImageClassifierConfig, run_inference
};

define_cli! { ARGS [about="run image classifier model for given configuration and input image"] =
    dry_run: bool [help="just load and display model - don't run inference", long],
    config: PathBuf [help="path for image classfier configuration file to use", short, long],
    img: PathBuf [help="input image to classify"]
}

fn main() -> Result<()> {
    let config: ImageClassifierConfig = ron::from_path(&ARGS.config)?;
    let mut session = Session::builder()?.commit_from_file(&config.model_path)?;
    print_session_info( &session);

    let img = image::open(&ARGS.img)?.to_rgb8();
    let width = img.width() as usize;
    let height = img.height() as usize;

    let imgs = fit( &img, &config)?;
    if !ARGS.dry_run {
        for img in &imgs {
            let (model_w, model_h) = img.dimensions();
            let img_data: Array4<f32> = img_to_array4(img);
            let inputs = ort::inputs![ "images" => Tensor::from_array(img_data)? ];

            run_inference( &mut session, inputs, |outputs| {
                println!("inference output");
                let output0 = outputs["output0"].try_extract_array::<f32>()?.t().into_owned();
                for row in output0.axis_iter( Axis(0)) {
                    let row: Vec<_> = row.iter().copied().collect();
                    let probability = row[4];

                    if probability > 0.1 {
                        println!("{:?}", row);
                    }
                }

                Ok(())
            })?;
        }
    }

    Ok(())
}
