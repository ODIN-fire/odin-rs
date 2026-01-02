#![allow(unused)]

use anyhow::{Result};
use image::{DynamicImage,GenericImageView};
use odin_common::define_cli;
use odin_image::{
    filtered_rgb_stats, get_dominant_tile_size, get_grid_dim, get_horizon, max_rgba,
    Mask
};
use odin_image::{Result as OdinImageResult};

define_cli! { ARGS [about="compute gray-/white-ness factors of tiles from given image"] =
    top_margin: u32 [help="optional top margin to crop from input image", long, default_value="0"],
    bottom_margin: u32 [help="optional bottom margin to crop from input image", long, default_value="0"],

    //--- tile grid parameters
    fractional_tiles: bool [help="process fractional tiles at right/bottom image", long],
    n: usize [help="number of tiles", long, short, default_value="10"],
    vertical: bool [help="tile vertically",long],
    keep_ratio: bool [help="use aspect-ratio tile sizes (default is square)",long],

    //--- horizon detection parameters
    y_dist: u32 [help="horizontal distance in pixels to determine V,S gradients", long, default_value="20"],
    v_diff: f32 [help="min V difference (gradient) we consider to be the horizon line [0..-1]", long, default_value="-0.1"],
    s_diff: f32 [help="min S difference (gradient) we consider to be the horizon line [0..1]", long, default_value="0.1"],
    loess_width: usize [help="bandwidth for LOESS smoothing of horizon edge", long, default_value="20"],
    offset: i32 [help="horizon offset in pixels", long, allow_hyphen_values = true, default_value="0"],

    low: f32 [help="lower outlier percentage [0..1]", long, default_value="0.1"],
    high: f32 [help="upper outlier percentage [0..1", long, default_value="0.1"],

    mask: Option<String> [help="optional mask file to use", long],
    horizon: Option<String> [help="optional filename for horizon (JSON) file", long],

    src_file: String [help="filename of image to analyze"]
}

fn main() -> Result<()> {
    let img = image::open(&ARGS.src_file)?;
    let horizon = get_horizon( ARGS.horizon.as_ref(), &img, ARGS.top_margin, ARGS.y_dist, ARGS.v_diff, ARGS.s_diff, ARGS.loess_width)?;

    let (tile_width, tile_height) = get_dominant_tile_size( &img, ARGS.n, !ARGS.vertical, ARGS.keep_ratio);
    let (nx,ny) = get_grid_dim( &img, tile_width, tile_height, ARGS.fractional_tiles);
    let mask = Mask::maybe_open_checked( ARGS.mask.as_ref(), nx, ny)?;

    let valid_pixel_pred = |x: u32, y: u32, rgba: &[u8;4]| {
        y >= horizon[x as usize]
        && max_rgba( rgba) < 220 // saturation
    };

    let rgb_stats = filtered_rgb_stats( &img, tile_width, tile_height, ARGS.fractional_tiles, mask.as_ref(), &valid_pixel_pred)?;
    rgb_stats.print( ARGS.low, ARGS.high);

    Ok(())
}