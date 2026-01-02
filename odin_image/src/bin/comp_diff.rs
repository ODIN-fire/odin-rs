use odin_common::define_cli;
use odin_image::{
    blackout_sky, compressed_size, get_tiled_comp, get_dominant_tile_size, get_grid_dim, get_horizon, 
    open_diff_image_pair, Mask
};
use anyhow::{Result};


define_cli! { ARGS [about="compute complexity diffs for tiles from two images"] =
    top_margin: u32 [help="optional top margin to crop from input image", long, default_value="0"],
    bottom_margin: u32 [help="optional bottom margin to crop from input image", long, default_value="0"],

    //--- tiling parameters
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

    mask: Option<String> [help="optional mask file to use", long],
    horizon: Option<String> [help="optional filename for horizon (JSON) file", long],

    src_file1: String [help="filename of first image to compare"],
    src_file2: String [help="filename of second image to compare"]
}

fn main()->Result<()> {
    let (mut img1, mut img2) = open_diff_image_pair( &ARGS.src_file1, &ARGS.src_file2)?;
    let horizon = get_horizon( ARGS.horizon.as_ref(), &img1, ARGS.top_margin, ARGS.y_dist, ARGS.v_diff, ARGS.s_diff, ARGS.loess_width)?;

    // make sure sky doesn't contribute to complexity difference
    blackout_sky(&mut img1, &horizon)?;
    blackout_sky(&mut img2, &horizon)?;

    // TODO - remove bottom-margin before compression

    if ARGS.n == 0 {
        let d1 = compressed_size( &img1);
        let d2 = compressed_size( &img2);
        let d = (d2-d1) as f32 / d1 as f32; // relative change of compressed size

        println!("{:9.4}", d);

    } else {
        let (tile_width, tile_height) = get_dominant_tile_size( &img1, ARGS.n, !ARGS.vertical, ARGS.keep_ratio);
        let (nx,ny) = get_grid_dim( &img1, tile_width, tile_height, ARGS.fractional_tiles);
        let mask = Mask::maybe_open_checked( ARGS.mask.as_ref(), nx, ny)?;

        let d1 = get_tiled_comp( &img1, tile_width, tile_height, ARGS.fractional_tiles, mask.as_ref())?;
        let d2 = get_tiled_comp( &img2, tile_width, tile_height, ARGS.fractional_tiles, mask.as_ref())?;

        let diff = d2.rel_diff( &d1)?;

        diff.print( 6, 2);
    }

    Ok(())
}