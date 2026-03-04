/*
 * Copyright © 2026, United States Government, as represented by the Administrator of
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

use std::{fs,collections::HashMap};
use kml::{Kml, KmlReader, types::{Coord, Geometry, MultiGeometry}};
use kiddo::{KdTree, NearestNeighbour, SquaredEuclidean};
use rkyv::{ to_bytes, {rancor::Error as RkyvError}, Archive, Serialize};
use anyhow::{anyhow,Result};

use odin_common::{define_cli,cartographic::Cartographic,cartesian3::Cartesian3};
use odin_orbital::copernicus::{tile_enc};

define_cli! { ARGS [about="create ESA Sentinel-2 tile kd-tree for tile ECEF positions"] =
    boundaries: Option<String> [help="optional filename to store HashMap with tile boundaries", long],
    input: String [help="filename of Sentinel-2 KMZ input"],
    output: String [help="filename for (rkyv) serialized kd-tree"]
}

fn main()->Result<()> {
    let mut kdtree: KdTree<f64,3> = KdTree::new();
    let mut boundaries: HashMap<String,[[f64;2];4]> = HashMap::new();

    let res = if ARGS.input.ends_with(".kmz") {
        KmlReader::<_,f64>::from_kmz_path(&ARGS.input)?.read()
    } else {
        KmlReader::<_,f64>::from_path(&ARGS.input)?.read()
    };

    if let Ok(Kml::KmlDocument(doc)) = res {
        if let Some(Kml::Document{attrs,elements}) = doc.elements.first() {
            for e in elements {
                if let Kml::Folder(folder) = e {
                    if let Some(name) = &folder.name {
                        if name == "Features" {
                            for e in &folder.elements {
                                if let Kml::Placemark(placemark) = e
                                && let Some(tile_id) = placemark.name.as_ref()
                                && let Some(Geometry::MultiGeometry(mgeo)) = &placemark.geometry {
                                    for g in &mgeo.geometries {
                                        match g {
                                            Geometry::Point(pt) => {
                                                let Coord{x,y,z} = pt.coord;
                                                let geo = Cartographic::from_degrees( x, y, 0.0);
                                                let ecef: Cartesian3 = geo.into();
                                                kdtree.add( &[ecef.x,ecef.y,ecef.z], tile_enc(&tile_id));
                                                break; // we only need the poly preceding the point
                                            }
                                            Geometry::Polygon(poly) => {
                                                if ARGS.boundaries.is_some() {
                                                    let coords = &poly.outer.coords;
                                                    if coords.len() >= 4 {
                                                        let p0 = [coords[0].x, coords[0].y];
                                                        let p1 = [coords[1].x, coords[1].y];
                                                        let p2 = [coords[2].x, coords[2].y];
                                                        let p3 = [coords[3].x, coords[3].y];
                                                        boundaries.insert( tile_id.clone(), [p0,p1,p2,p3]);
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let bytes = to_bytes::<RkyvError>(&kdtree)?;
    fs::write( &ARGS.output, &bytes)?;
    println!("created tile kdtree {} ({} tile entries)", ARGS.output, kdtree.size());

    if let Some(path) = &ARGS.boundaries {
        let bytes = to_bytes::<RkyvError>(&boundaries)?;
        fs::write( path, &bytes)?;
        println!("created tile boundaries HashMap {} ({} tile entries)", path, boundaries.len());
    }

    Ok(())
}
