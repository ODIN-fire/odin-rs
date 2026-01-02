
#![allow(unused)]

use csv;
use std::{fs::File, io::Read};
use odin_himawari::{read_hotspots,errors::Result};

/// run with "cargo test --test test_read test_csv -- --nocapture"

#[test]
pub fn test_csv ()->Result<()> {
    let mut file = File::open("resources/H09_20251209_1900_L2WLF010_FLDK.06001_06001.csv")?;
    let hs = read_hotspots(file)?;

    for h in &hs { println!("{h:?}") }

    Ok(())
}
