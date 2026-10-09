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

//! tif_viewer — a simple interactive TIFF raster viewer
//! TODO - add lower/upper percentile to handle outliers

use anyhow::{bail, Context, Result, anyhow};
use gdal::raster::GdalType;
use gdal::Dataset;
use minifb::{Key, MouseButton, MouseMode, Window, WindowOptions};
use std::{path::PathBuf};

use odin_common::{define_cli, fs::path_as_str};

define_cli! { ARGS [about="interactive TIF raster data viewer"] =
    input: PathBuf [help="path to input *.tif file"],
    band: usize [help="raster band to display", long, default_value="1"],
    width: usize [help="window width in pixel", long, default_value="900"],
    height: usize [help="window height in pixel", long, default_value="700"],
    border: bool [help="border width to draw around image", short, long],
    border_color: String [help="border color (0xRRGGBB) to draw around image", long, default_value="0xffffff"],
    lower_limit: f64 [help="lower cutoff for outliers [0.0..1.0]", long, default_value="0.0"],
    upper_limit: f64 [help="upper cutoff for outliers [0.0..1.0]", long, default_value="1.0"],
    tint: String [help="image tint color (0xRRGGBB)", long, default_value="0xff0000"]
}

fn main() -> Result<()> {
    let border_color_rgb = parse_hex_color( &ARGS.border_color)?;
    let tint_rgb = parse_hex_color( &ARGS.tint)?;
    let border_color_u32 = rgb_to_u32( border_color_rgb.0, border_color_rgb.1, border_color_rgb.2);

    let ds = Dataset::open( &ARGS.input).with_context(|| format!("Cannot open {:?}", ARGS.input))?;
    let raster = RasterData::from_dataset(&ds, ARGS.band)?;

    let mapper = LinearColorMapper::new(tint_rgb);
    let mut view = ViewState::fit(&raster, ARGS.width, ARGS.height, ARGS.border);

    let title = format!( "tif_viewer — {} (band {})", path_as_str( &ARGS.input.file_name().ok_or(anyhow!("no filename"))?)?, ARGS.band);
    let mut window = Window::new( &title, ARGS.width, ARGS.height, WindowOptions { resize: true, ..WindowOptions::default() })
        .context("Failed to open display window")?;

    window.set_target_fps(60);

    let mut drag_last: Option<(f32, f32)> = None;
    let prev_wheel: f32 = 0.0;

    let mut frame_buf = render_frame( &raster, &view, &mapper, border_color_u32);
    let mut dirty = true;

    println!("Controls: arrows=pan  +/-=zoom  wheel=zoom  drag=pan  R=reset  S=save  Q/Esc=quit");

    while window.is_open() {
        let (ww, wh) = window.get_size();

        //--- got resized
        if ww != view.win_w || wh != view.win_h {
            // Keep the raster centre fixed on resize
            let old_cx = view.origin_x + view.inner_w() as f64 / (2.0 * view.zoom);
            let old_cy = view.origin_y + view.inner_h() as f64 / (2.0 * view.zoom);
            view.win_w = ww;
            view.win_h = wh;
            view.origin_x = old_cx - view.inner_w() as f64 / (2.0 * view.zoom);
            view.origin_y = old_cy - view.inner_h() as f64 / (2.0 * view.zoom);

            frame_buf = vec![0u32; ww * wh];
            dirty = true;
        }

        const PAN_PX: f64 = 40.0;   // pixels per arrow key press
        const ZOOM_F: f64 = 1.25;   // zoom factor per key press

        if window.is_key_pressed(Key::Escape, minifb::KeyRepeat::No)
            || window.is_key_pressed(Key::Q, minifb::KeyRepeat::No)
        {
            break;
        }
        if window.is_key_down(Key::Left) {
            view.pan(-PAN_PX, 0.0);
            dirty = true;
        }
        if window.is_key_down(Key::Right) {
            view.pan(PAN_PX, 0.0);
            dirty = true;
        }
        if window.is_key_down(Key::Up) {
            view.pan(0.0, -PAN_PX);
            dirty = true;
        }
        if window.is_key_down(Key::Down) {
            view.pan(0.0, PAN_PX);
            dirty = true;
        }
        if window.is_key_pressed(Key::Equal, minifb::KeyRepeat::Yes)   // '+'
            || window.is_key_pressed(Key::NumPadPlus, minifb::KeyRepeat::Yes)
        {
            view.zoom_center(ZOOM_F);
            dirty = true;
        }
        if window.is_key_pressed(Key::Minus, minifb::KeyRepeat::Yes)
            || window.is_key_pressed(Key::NumPadMinus, minifb::KeyRepeat::Yes)
        {
            view.zoom_center(1.0 / ZOOM_F);
            dirty = true;
        }
        if window.is_key_pressed(Key::R, minifb::KeyRepeat::No) {
            view = ViewState::fit(&raster, view.win_w, view.win_h, (view.border > 0) );
            dirty = true;
        }
        if window.is_key_pressed(Key::S, minifb::KeyRepeat::No) {
            let path = format!( "{}_view.png", ARGS.input.file_stem().unwrap_or_default().to_string_lossy());
            if let Err(e) = save_png( &frame_buf, view.win_w, view.win_h, &path) {
                eprintln!("Save failed: {e}");
            }
        }

        //--- mouse zoom
        if let Some(scroll) = window.get_scroll_wheel() {
            if scroll.1 != 0.0 {
                if let Some((mx, my)) = window.get_mouse_pos(MouseMode::Clamp) {
                    let factor = if scroll.1 > 0.0 { ZOOM_F } else { 1.0 / ZOOM_F };
                    view.zoom_at( factor, mx as f64, my as f64);
                    dirty = true;
                }
            }
        }

        //--- mouse pan (Dragging moves the viewport opposite to drag direction)
        if window.get_mouse_down(MouseButton::Left) {
            if let Some((mx, my)) = window.get_mouse_pos(MouseMode::Clamp) {
                if let Some((lx, ly)) = drag_last {
                    let dx = mx as f64 - lx as f64;
                    let dy = my as f64 - ly as f64;
                    if dx != 0.0 || dy != 0.0 {
                        view.pan(-dx, -dy);
                        dirty = true;
                    }
                }
                drag_last = Some((mx, my));
            }
        } else {
            drag_last = None;
        }

        if dirty {
            frame_buf = render_frame( &raster, &view, &mapper, border_color_u32);
            dirty = false;
        }

        window
            .update_with_buffer( &frame_buf, view.win_w, view.win_h)
            .context("Failed to update window buffer")?;
    }

    Ok(())
}

fn parse_hex_color(s: &str) -> Result<(u8, u8, u8)> {
    let s = s.trim_start_matches("0x").trim_start_matches("0X").trim_start_matches('#');
    let v = u32::from_str_radix(s, 16).with_context(|| format!("Invalid hex colour: {s}"))?;
    Ok(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// Pack RGB into minifb's 0x00RRGGBB u32 format.
#[inline(always)]
fn rgb_to_u32(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}


/// Render one frame into a `Vec<u32>` ARGB buffer suitable for minifb.
/// - The inner rectangle is filled pixel-by-pixel by sampling the raster
///   at the raster coordinate that maps to each screen pixel (nearest-neighbour).
fn render_frame<T> (raster: &RasterData, view: &ViewState, mapper: &T, border_color: u32) -> Vec<u32>
    where T: ColorMapper
{
    let w = view.win_w;
    let h = view.win_h;
    let border = view.border;

    let mut buf = vec![0u32; w*h];

    let inner_w = view.inner_w();
    let inner_h = view.inner_h();

    let data_range = raster.data_max - raster.data_min;
    let inv_range = if data_range > 0.0 { 1.0 / data_range as f64 } else { 1.0 };

    let ri_max = raster.rows as isize;
    let ci_max = raster.cols as isize;

    let nodata_color = {
        let (r, g, b) = mapper.nodata_color();
        rgb_to_u32(r, g, b)
    };

    for py in 0..inner_h {
        let ry = view.origin_y + py as f64 / view.zoom;
        let ri = ry.floor() as isize; // raster row

        let row_offset = (py + border) * w + border;

        for px in 0..inner_w {
            let rx = view.origin_x + px as f64 / view.zoom;
            let ci = rx.floor() as isize; // raster col

            let pixel = if (ri < 0 || ri >= ri_max) || (ci < 0 || ci >= ci_max) { // fb pixel outside image
                nodata_color
            } else {
                let idx = ri as usize * raster.cols + ci as usize;
                let v = raster.values[idx];
                if v == raster.nodata {
                    nodata_color
                } else {
                    let t = ((v as f64 - raster.data_min as f64) * inv_range) as f32;
                    let (r, g, b) = mapper.map_normalized(t.clamp(0.0, 1.0));
                    rgb_to_u32(r, g, b)
                }
            };

            buf[row_offset + px] = pixel;
        }
    }

    if border > 0 {
        draw_border( &mut buf, &view, &raster, border, border_color)
    }

    buf
}

fn draw_border (buf: &mut[u32], view: &ViewState, raster: &RasterData, border: usize, border_color: u32) {
    let vw = view.win_w as isize;
    let vh = view.win_h as isize;

    let x0 = (-view.origin_x * view.zoom).floor() as isize;
    let y0 = (-view.origin_y * view.zoom).floor() as isize;

    let w = (raster.cols as f64 * view.zoom).floor() as isize;
    let h = (raster.rows as f64 * view.zoom).floor() as isize;

    let x1 = x0 + w;
    let y1 = y0 + h;

    if x0 < vw && x1 > 0 { // top and bottom border
        let x0 = x0.max(0);
        let x1 = x1.min(vw);

        if y0 >= 0 && y0 < vh {
            let i0 = y0 * vw;
            for x in x0..x1 { buf[ (i0 + x) as usize ] = border_color; }
        }

        if y1 >= 0 && y1 < vh {
            let i0 = y1 * vw;
            for x in x0..x1 { buf[ (i0 + x) as usize ] = border_color; }
        }
    }

    if y0 < vh && y1 > 0 { // left and right border
        let y0 = y0.max(0);
        let y1 = y1.min(vh);

        if x0 >= 0 && x0 < vw {
            for y in y0..y1 {
                let i = (y * vw + x0) as usize;
                buf[i] = border_color;
            }
        }

        if x1 >= 0 && x1 < vw {
            for y in y0..y1 {
                let i = (y * vw + x1) as usize;
                buf[i] = border_color;
            }
        }
    }
}

//--- input raster data

/// Band data normalised to f32; nodata pixels are flagged.
pub struct RasterData {
    pub values: Vec<f32>,   // row-major, [row * cols + col]
    pub nodata: f32,  // nodata value of input band (or NAN if none set)
    pub cols: usize,
    pub rows: usize,
    pub data_min: f32,
    pub data_max: f32,
}

impl RasterData {
    /// Read band `band_idx` (1-based) from a GDAL dataset into an f32 buffer.
    pub fn from_dataset(ds: &Dataset, band_idx: usize) -> Result<Self> {
        let band = ds.rasterband(band_idx)
            .with_context(|| format!("Cannot open band {band_idx}"))?;

        let (cols, rows) = band.size();
        let nodata = band.no_data_value().unwrap_or( f64::NAN) as f32;

        // Read using f64 then convert; this handles both i16 and f32 source types
        // transparently through GDAL's built-in type conversion.
        let buf = band
            .read_as::<f64>(
                (0, 0),
                (cols, rows),
                (cols, rows),
                None,
            )
            .context("Failed to read raster band")?;

        let raw: Vec<f64> = buf.data().to_vec();
        let values: Vec<f32> = raw.iter().map(|&v| v as f32).collect();

        // Statistics over valid pixels only
        let mut mn = f32::MAX;
        let mut mx = f32::MIN;
        for v in values.iter() {
            if *v != nodata {
                if *v < mn { mn = *v; }
                if *v > mx { mx = *v; }
            }
        }
        if mn > mx { mn = 0.0; mx = 1.0; } // all nodata fallback

        println!("Band {band_idx}: {cols}×{rows} pixels, value range [{mn:.4}, {mx:.4}]");

        Ok(Self { values, nodata, cols, rows, data_min: mn, data_max: mx })
    }
}

//--- color mapping

/// Maps a scalar raster value in [data_min, data_max] (or nodata) to an RGB triplet.
/// Implementors only need to provide `map_value`; the viewer calls it per pixel.
pub trait ColorMapper {
    /// Map a valid raster value (already clamped to [0,1] normalised range) to RGB.
    /// `t` is the linearly normalised position: 0.0 = data_min, 1.0 = data_max.
    fn map_normalized(&self, t: f32) -> (u8, u8, u8);

    /// Color used for nodata / out-of-range pixels.
    fn nodata_color(&self) -> (u8, u8, u8) { (30, 30, 50) }
}

/// Linearly scales data value range → black (0,0,0) to `tint` (r,g,b).
pub struct LinearColorMapper {
    /// Target color at full intensity (value = data_max)
    pub tint: (u8, u8, u8),
}

impl LinearColorMapper {
    pub fn new(tint: (u8, u8, u8)) -> Self { Self { tint } }
    pub fn greyscale() -> Self { Self::new((255, 255, 255)) }
}

impl ColorMapper for LinearColorMapper {

    #[inline(always)]
    fn map_normalized(&self, t: f32) -> (u8, u8, u8) {
        let t = t.clamp(0.0, 1.0);
        let r = (self.tint.0 as f32 * t) as u8;
        let g = (self.tint.1 as f32 * t) as u8;
        let b = (self.tint.2 as f32 * t) as u8;
        (r, g, b)
    }
}

//── view  state

/// Tracks the current pan/zoom viewport.
///
/// The raster's coordinate space has origin (0,0) at top-left with axes in pixels.
/// `origin_x / origin_y` is the raster coordinate shown at the top-left of the
/// *inner* display area (window minus borders).
/// `zoom` is pixels-per-raster-pixel (> 1 = zoomed in, < 1 = zoomed out).
pub struct ViewState {
    pub origin_x: f64,  // raster col at inner top-left corner
    pub origin_y: f64,  // raster row at inner top-left corner
    pub zoom: f64,       // screen pixels per raster pixel
    pub win_w: usize,
    pub win_h: usize,
    pub border: usize,
}

impl ViewState {
    /// Compute zoom and origin so the entire raster fits the inner area.
    pub fn fit(raster: &RasterData, win_w: usize, win_h: usize, draw_border: bool) -> Self {
        let border = if draw_border { 1 } else { 0 };

        let inner_w = win_w.saturating_sub(2 * border) as f64;
        let inner_h = win_h.saturating_sub(2 * border) as f64;
        let zoom = (inner_w / raster.cols as f64).min(inner_h / raster.rows as f64);

        // center raster in the inner area
        let shown_w = raster.cols as f64 * zoom;
        let shown_h = raster.rows as f64 * zoom;
        let origin_x = -(inner_w - shown_w) / (2.0 * zoom);
        let origin_y = -(inner_h - shown_h) / (2.0 * zoom);
        Self { origin_x, origin_y, zoom, win_w, win_h, border }
    }

    #[inline(always)]
    pub fn inner_w(&self) -> usize { self.win_w.saturating_sub(2 * self.border) }

    #[inline(always)]
    pub fn inner_h(&self) -> usize { self.win_h.saturating_sub(2 * self.border) }

    /// Convert a screen pixel (relative to window top-left) to raster coords.
    #[inline(always)]
    pub fn screen_to_raster(&self, sx: f64, sy: f64) -> (f64, f64) {
        let rx = self.origin_x + (sx - self.border as f64) / self.zoom;
        let ry = self.origin_y + (sy - self.border as f64) / self.zoom;
        (rx, ry)
    }

    /// Zoom by `factor` keeping the given screen point fixed in place.
    pub fn zoom_at(&mut self, factor: f64, screen_x: f64, screen_y: f64) {
        let (rx, ry) = self.screen_to_raster(screen_x, screen_y);
        self.zoom = (self.zoom * factor).clamp(0.05, 200.0);

        // Recompute origin so (rx,ry) stays at (screen_x, screen_y)
        self.origin_x = rx - (screen_x - self.border as f64) / self.zoom;
        self.origin_y = ry - (screen_y - self.border as f64) / self.zoom;
    }

    /// Pan by `(dx, dy)` screen pixels.
    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.origin_x += dx / self.zoom;
        self.origin_y += dy / self.zoom;
    }

    /// Zoom centred on the inner-area centre.
    pub fn zoom_center(&mut self, factor: f64) {
        let cx = self.border as f64 + self.inner_w() as f64 / 2.0;
        let cy = self.border as f64 + self.inner_h() as f64 / 2.0;
        self.zoom_at(factor, cx, cy);
    }
}

/// export current view to PNG
fn save_png(buf: &[u32], w: usize, h: usize, path: &str) -> Result<()> {
    let mut img = image::RgbImage::new(w as u32, h as u32);
    for (i, &px) in buf.iter().enumerate() {
        let r = ((px >> 16) & 0xFF) as u8;
        let g = ((px >>  8) & 0xFF) as u8;
        let b = (px         & 0xFF) as u8;
        let x = (i % w) as u32;
        let y = (i / w) as u32;
        img.put_pixel(x, y, image::Rgb([r, g, b]));
    }
    img.save(path)?;
    println!("Saved {path}");
    Ok(())
}
