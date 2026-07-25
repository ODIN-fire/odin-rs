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
#![allow(unused)]

use anyhow::Result;
use image::Rgb;
use imageproc::{drawing::{Canvas,draw_hollow_rect_mut, draw_text_mut},rect::Rect};
use odin_common::{define_cli};
use odin_image::get_hex_rgb;


define_cli! { ARGS [about="draw frame around RGB image"] =
    color: String [help="frame color (hex rrggbb)", long, default_value="00ffff"],
    width: u32 [help="width of frame", long, default_value="1"],
    img_file: String [help="filename of input image"]
}

fn main() -> Result<()> {
    let img = image::open(&ARGS.img_file)?;
    let (w,h) = img.dimensions();
    let clr = Rgb(get_hex_rgb(&ARGS.color));

    let mut img = img.to_rgb8();

    for i in 0..ARGS.width {
        let rect = Rect::at(i as i32,i as i32).of_size( (w - i*2), (h - i*2));
        draw_hollow_rect_mut( &mut img, rect, clr);
    }

    img.save(&ARGS.img_file)?;

    Ok(())
}
