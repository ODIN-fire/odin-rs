/*
 * Copyright (c) 2024, United States Government, as represented by the
 * Administrator of the National Aeronautics and Space Administration.
 * All rights reserved.
 *
 * The ODIN - Open Data Integration Framework is licensed under the
 * Apache License, Version 2.0 (the "License"); you may not use this file
 * except in compliance with the License. You may obtain a copy of the
 * License at http://www.apache.org/licenses/LICENSE-2.0.
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
use ron;
use anyhow::Result;
use odin_server::imagery_service::Imagery;

#[test]
fn test_arcgis()->Result<()> {
    let input = r#"
    #![enable(implicit_some)]
    Imagery(
        pathname: "globe/natgeo",
        info: "global ArcGIS NatGeo Terrain map",
        exclusive: ["globe"],
        provider: ArcGisMapServerImageryProvider( uri: "https://services.arcgisonline.com/ArcGIS/rest/services/NatGeo_World_Map/MapServer/"),
        proxy: true,
        show: true,
        rendering: ImageryRenderingParams (
            brightness: 0.6
        )
    )
    "#;
    let imagery: Imagery = ron::from_str(input)?;
    println!("{:#?}", imagery); 

    Ok(())
}

#[test]
fn test_tms()->Result<()> {
    let input = r#"
    #![enable(implicit_some)]
    Imagery(
        pathname: "goes18/geocolor",
        info: "latest GOES-18 geocolor imagery",
        exclusive: [],
        provider: TileMapServiceImageryProvider(
             uri: "https://services.arcgisonline.com/ArcGIS/rest/services/NatGeo_World_Map/MapServer/",
             bounds: (
                west: -121.9, south: 36.9, east: -121.5, north: 37.1
             )
        ),
        proxy: true,
        show: true,
        rendering: ImageryRenderingParams (
            brightness: 0.6
        )
    )
    "#;
    let imagery: Imagery = ron::from_str(input)?;
    println!("{:#?}", imagery); 

    Ok(())
}

#[test]
fn test_wmts()->Result<()> {
    let input = r#"
    #![enable(implicit_some)]
    Imagery(
        pathname: "satellite/truecolor/terra",
        info: "MODIS Terra truecolor",
        exclusive: [],
        provider: WebMapTileServiceImageryProvider(
             uri: "https://gibs.earthdata.nasa.gov/wmts/epsg4326/best/MODIS_Terra_CorrectedReflectance_TrueColor/default/2022-10-10/{TileMatrixSet}/{TileMatrix}/{TileRow}/{TileCol}.jpg",
             params: {
                "layer": String("MODIS_Terra_CorrectedReflectance_TrueColor"),
                "style": String("default"),
                "tileMatrixSetID": String("250m"),
                "maximumLevel": Int(5),
                "format": String("image/jpeg")
             }
        ),
        proxy: true,
        show: true,
        rendering: ImageryRenderingParams (
            brightness: 0.6
        )
    )
    "#;
    let imagery: Imagery = ron::from_str(input)?;
    println!("{:#?}", imagery); 

    Ok(())
}