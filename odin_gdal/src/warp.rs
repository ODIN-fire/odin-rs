/*
 * Copyright © 2024, United States Government, as represented by the Administrator of 
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
use std::ffi::{CString,CStr};
use std::path::{Path, PathBuf};
use std::fs;
use std::ptr::{null,null_mut};
use std::error::Error;
use gdal::raster::GdalDataType;
use gdal::{DriverManager,Dataset,DatasetOptions, GeoTransform};
use gdal::cpl::CslStringList;
use gdal::spatial_ref::SpatialRef;
use gdal_sys::{GDALDatasetH, GDALProgressFunc, GDALWarpOptions, CPLErr::CE_None, CPLErr, GDALResampleAlg};
use libc::{c_void,c_char,c_int, c_double, c_uint};
use bit_set::BitSet;
use odin_common::{abs,BoundingBox,geo::GeoRect};
use crate::{ok_non_null, ok_mut_non_null, ok_not_zero, ok_ce_none, RasterInfo};
use crate::errors::{Result,last_gdal_error, misc_error, OdinGdalError, reset_last_gdal_error};

#[derive(Clone)]
pub enum ResampleAlg {
    NearestNeighbour = GDALResampleAlg::GRA_NearestNeighbour as isize,
    Bilinear         = GDALResampleAlg::GRA_Bilinear as isize,
    Cubic            = GDALResampleAlg::GRA_Cubic as isize,
    CubicSpline      = GDALResampleAlg::GRA_CubicSpline as isize,
    Lanczos          = GDALResampleAlg::GRA_Lanczos as isize,
    Average          = GDALResampleAlg::GRA_Average as isize,
    Mode             = GDALResampleAlg::GRA_Mode as isize,
    Max              = GDALResampleAlg::GRA_Max as isize,
    Min              = GDALResampleAlg::GRA_Min as isize,
    Med              = GDALResampleAlg::GRA_Med as isize,
    Q1               = GDALResampleAlg::GRA_Q1 as isize,
    Q3               = GDALResampleAlg::GRA_Q3 as isize,
    Sum              = GDALResampleAlg::GRA_Sum as isize,
    RMS              = GDALResampleAlg::GRA_RMS as isize,
    //LastValue        = GDALResampleAlg::GRA_LAST_VALUE as isize  // NOTE - LastValue is the same as RMS
}



pub struct SimpleWarpBuilder <'a> {
    src_ds: &'a Dataset,
    tgt_filename: CString,

    min_x: c_double,
    max_x: c_double,
    min_y: c_double,
    max_y: c_double,

    res_x: c_double,
    res_y: c_double,

    force_n_lines: c_int,
    force_n_pixels: c_int,

    tgt_srs: Option<&'a SpatialRef>,
    tgt_format: Option<CString>,
    create_options: Option<&'a CslStringList>,
    src_srs: Option<&'a SpatialRef>,

    n_tgt_bands: Option<c_uint>,
    extra_tgt_bands: usize,           // number of un-initialized extra bands to add to tgt dataset
    src_bands: Option<Vec<c_uint>>,
    tgt_bands: Option<Vec<c_uint>>,

    data_type: Option<GdalDataType>,
    src_nodatas: Option<Vec<c_double>>,
    tgt_nodatas: Option<Vec<c_double>>,

    axis_order: c_int,
    max_error: c_double,
    resample_alg: ResampleAlg,
}

impl <'a> SimpleWarpBuilder<'a> {
    pub fn new <P: AsRef<Path>>(src_ds: &'a Dataset, tgt: P) -> Result<SimpleWarpBuilder<'a>> {
        let path = tgt.as_ref();
        let tgt_str = path.to_str().ok_or(OdinGdalError::InvalidFileName(path.display().to_string()))?;
        let tgt_filename = CString::new(tgt_str)?;

        Ok(SimpleWarpBuilder {
            src_ds,
            tgt_filename,

            min_x: 0.0, max_x: 0.0, min_y: 0.0, max_y: 0.0,
            res_x: 0.0, res_y: 0.0,
            force_n_lines: 0, force_n_pixels: 0,

            tgt_srs: None,
            tgt_format: None,
            create_options: None,
            src_srs: None,

            n_tgt_bands: None, // compute number of target bands
            extra_tgt_bands: 0,

            src_bands: None, // means process all bands
            tgt_bands: None,

            data_type: None,
            src_nodatas: None,
            tgt_nodatas: None,

            axis_order: 0,
            max_error: 0.0,
            resample_alg: ResampleAlg::NearestNeighbour,
        })
    }

    pub fn set_tgt_extent (&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> &mut SimpleWarpBuilder<'a> {
        self.min_x = min_x;
        self.max_x = max_x;
        self.min_y = min_y;
        self.max_y = max_y;
        self
    }

    pub fn set_tgt_extent_from_bbox (&mut self, bbox: &BoundingBox<f64>) ->  &mut SimpleWarpBuilder<'a> {
        self.min_x = bbox.west;
        self.max_x = bbox.east;
        self.min_y = bbox.south;
        self.max_y = bbox.north;
        self
    }

    pub fn set_tgt_extent_from_rect (&mut self, bbox: &GeoRect) ->  &mut SimpleWarpBuilder<'a> {
        self.min_x = bbox.west().degrees();
        self.max_x = bbox.east().degrees();
        self.min_y = bbox.south().degrees();
        self.max_y = bbox.north().degrees();
        self
    }

    pub fn set_tgt_resolution (&mut self, res_x: f64, res_y: f64) -> &mut SimpleWarpBuilder<'a> {
        self.res_x = res_x;
        self.res_y = res_y;
        self
    }

    pub fn set_tgt_size (&mut self, npixels: i32, nlines: i32) -> &mut SimpleWarpBuilder<'a> {
        self.force_n_pixels = npixels;
        self.force_n_lines = nlines;
        self
    }

    pub fn set_tgt_srs (&mut self, srs: &'a SpatialRef) -> &mut SimpleWarpBuilder<'a> {
        self.tgt_srs = Some(srs);
        self
    }

    pub fn set_src_srs (&mut self, srs: &'a SpatialRef) -> &mut SimpleWarpBuilder<'a> {
        self.src_srs = Some(srs);
        self
    }

    // sets eResampleAlt
    pub fn set_resample_alg (&mut self, alg: ResampleAlg) -> &mut SimpleWarpBuilder<'a> {
        self.resample_alg = alg;
        self
    }

    pub fn set_src_bands (&mut self, src_bands: Vec<c_uint>) -> &mut SimpleWarpBuilder<'a> {
        self.src_bands = Some(src_bands);
        self
    }

    pub fn set_tgt_bands (&mut self, tgt_bands: Vec<c_uint>) -> &mut SimpleWarpBuilder<'a> {
        self.tgt_bands = Some(tgt_bands);
        self
    }

    pub fn set_extra_tgt_bands (&mut self, extra_tgt_bands: usize) -> &mut SimpleWarpBuilder<'a> {
        self.extra_tgt_bands = extra_tgt_bands;
        self
    }

    pub fn set_tgt_format (&mut self, tgt_format: &str) -> Result<&mut SimpleWarpBuilder<'a>> {
        self.tgt_format = Some(CString::new(tgt_format)?);
        Ok(self)
    }

    pub fn set_data_type (&mut self, data_type: GdalDataType) -> &mut SimpleWarpBuilder<'a> {
        self.data_type = Some(data_type);
        self
    }

    pub fn set_src_nodatas (&mut self, no_data_values: Vec<c_double>) -> &mut SimpleWarpBuilder<'a> {
        self.src_nodatas = Some(no_data_values);
        self
    } 

    pub fn set_tgt_nodatas (&mut self, no_data_values: Vec<c_double>) -> &mut SimpleWarpBuilder<'a> {
        self.tgt_nodatas = Some(no_data_values);
        self
    } 

    /// this has to be used to set compression, e.g. with "--co COMPRESS=DEFLATE --co PREDICTOR=2"
    pub fn set_create_options (&mut self, create_options: &'a CslStringList) -> &mut SimpleWarpBuilder<'a> {
        self.create_options = Some(create_options);
        self
    }

    pub fn set_axis_order (&mut self, order: i32) -> &mut SimpleWarpBuilder<'a> {
        self.axis_order = order;
        self
    }

    pub fn set_max_error (&mut self, max_error: f64) -> &mut SimpleWarpBuilder<'a> {
        self.max_error = max_error;
        self
    }

    // version without C shim functions

    pub fn exec(&self) -> Result<Dataset> {
        let mut tgt_ds = self.create_tgt_ds()?;
        self.chunk_and_warp( &mut tgt_ds).map(|_| tgt_ds)
    }

    fn create_tgt_ds (&self) -> Result<Dataset> {
        unsafe {
            reset_last_gdal_error();

            let c_src_ds = self.src_ds.c_dataset();
            let src_ds_srs = self.src_ds.spatial_ref().ok().or_else(|| self.src_ds.gcp_spatial_ref());
            let src_srs = self.src_srs.or_else(|| src_ds_srs.as_ref()).ok_or(OdinGdalError::NoSpatialReferenceSystem)?;

            let src_wkt = CString::new(src_srs.to_wkt()?)?;
            let tgt_srs = if let Some(srs_ref) = self.tgt_srs { srs_ref } else { src_srs };
            let tgt_wkt = CString::new(tgt_srs.to_wkt()?)?;

            let tgt_format = if let Some(format) = &self.tgt_format { format.as_ptr() } else { null() };
            let c_create_options = if let Some(sl) = self.create_options { sl.as_ptr() } else { null_mut() };

            // check if output file exists and if so delete it
            let path = Path::new(self.tgt_filename.to_str().unwrap()); // already checked during new()
            if path.is_file() { fs::remove_file(path)? }

            let c_driver = gdal_sys::GDALGetDriverByName(tgt_format);
            if c_driver == null_mut() {
                return Err(misc_error(format!("unknown output format {:?}", self.tgt_format)))
            }

            let mut geo_transform: [c_double; 6] = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
            let mut n_pixels: c_int = 0;
            let mut n_lines: c_int = 0;

            let c_transform_arg = ok_mut_non_null(
                gdal_sys::GDALCreateGenImgProjTransformer(c_src_ds, src_wkt.as_ptr(), null_mut(), tgt_wkt.as_ptr(), 1, 0.0, 0),
                || "GDALCreateGenImgProjTransformer() failed".to_string())?;

            if CE_None != gdal_sys::GDALSuggestedWarpOutput(c_src_ds, Some(gdal_sys::GDALGenImgProjTransform), c_transform_arg,
                                                            geo_transform.as_mut_ptr(), &mut n_pixels as *mut c_int, &mut n_lines as *mut c_int) {
                gdal_sys::GDALDestroyGenImgProjTransformer(c_transform_arg);
                return Err(last_gdal_error())
            }

            // sort so that min/max is east/north up
            let mut min_x = self.min_x;
            let mut max_x = self.max_x;
            let mut min_y = self.min_y;
            let mut max_y = self.max_y;

            if max_x < min_x { std::mem::swap( &mut max_x, &mut min_x) }
            if max_y < min_y { std::mem::swap( &mut max_y, &mut min_y) }

            let mut res_x = abs( self.res_x); // positive
            let mut res_y = - abs( self.res_y); // negative

            if res_x != 0.0 && res_y != 0.0 { // explicitly given pixel resolution
                if self.force_n_pixels != 0 || self.force_n_lines != 0 {
                    gdal_sys::GDALDestroyGenImgProjTransformer(c_transform_arg);
                    return Err(OdinGdalError::MiscError("cannot specify dimensions and resolution for warped dataset".to_string()))
                }

                // if we don't have explicit min/max values init from suggested transform
                if min_x == 0.0 && min_y == 0.0 && max_x == 0.0 && max_y == 0.0 {
                    min_x = geo_transform[0];
                    max_x = min_x + geo_transform[1] * n_pixels as c_double;
                    max_y = geo_transform[3];
                    min_y = max_y + geo_transform[5] * n_lines as c_double;
                }

                n_pixels = ((max_x - min_x + (res_x / 2.0)) / res_x).to_int_unchecked();
                n_lines = ((min_y - max_y + (res_y / 2.0)) / res_y).to_int_unchecked();  // res_y < 0

                // east/north up
                geo_transform[0] = min_x;
                geo_transform[1] = res_x;
                geo_transform[3] = max_y;
                geo_transform[5] = res_y;

            } else if self.force_n_pixels != 0 && self.force_n_lines != 0 { // explicitly given n_pixels, n_lines raster size
                if min_x == 0.0 && min_y == 0.0 && max_x == 0.0 && max_y == 0.0 {
                    min_x = geo_transform[0];
                    max_x = min_x + geo_transform[1] * n_pixels as c_double;
                    max_y = geo_transform[3];
                    min_y = max_y + geo_transform[5] * n_lines as c_double;
                }

                res_x = (max_x - min_x) / self.force_n_pixels as c_double;
                res_y = (min_y - max_y) / self.force_n_lines as c_double; // negative

                geo_transform[0] = min_x;
                geo_transform[1] = res_x;
                geo_transform[3] = max_y;
                geo_transform[5] = res_y;

                n_pixels = self.force_n_pixels;
                n_lines = self.force_n_lines;
                
            } else if min_x != 0.0 || min_y != 0.0 || max_x != 0.0 || max_y != 0.0 { // explicitly given min/max values
                res_x = geo_transform[1];
                res_y = geo_transform[5];

                n_pixels = ((max_x - min_x + (res_x / 2.0)) / res_x).to_int_unchecked();
                n_lines = ((min_y - max_y + (res_y / 2.0)) / res_y).to_int_unchecked();  // res_y < 0

                geo_transform[0] = min_x;
                geo_transform[3] = max_y;
            }

            let n_bands = self.get_n_tgt_bands( &self.src_ds)? + self.extra_tgt_bands as i32;
            let data_type: c_uint = if let Some(dt) = self.data_type {
                dt as c_uint
            } else {
                gdal_sys::GDALGetRasterDataType(gdal_sys::GDALGetRasterBand(c_src_ds, 1))
            };

            let c_tgt_ds = gdal_sys::GDALCreate(c_driver, self.tgt_filename.as_ptr(), n_pixels, n_lines, n_bands, data_type, c_create_options);
            if c_tgt_ds == null_mut() {
                let last_error = last_gdal_error();
                gdal_sys::GDALDestroyGenImgProjTransformer(c_transform_arg);
                return Err(last_error)
            }

            gdal_sys::GDALDestroyGenImgProjTransformer(c_transform_arg);

            gdal_sys::GDALSetProjection(c_tgt_ds, tgt_wkt.as_ptr());
            gdal_sys::GDALSetGeoTransform(c_tgt_ds, &mut geo_transform as *mut c_double);

            /* TODO this causes explicit no_data setting to fail and sets a '0' value for the target if the src doesn't have one
            // preserve no-data values and color tables
            for i in 1..=n_bands {
                let c_src_band = gdal_sys::GDALGetRasterBand(c_src_ds, i);
                let nv = gdal_sys::GDALGetRasterNoDataValue(c_src_band, null_mut());

                let c_tgt_band = gdal_sys::GDALGetRasterBand(c_tgt_ds, i);
                gdal_sys::GDALSetRasterNoDataValue(c_tgt_band, nv);

                let c_color_tbl = gdal_sys::GDALGetRasterColorTable(c_src_band);
                if c_color_tbl != null_mut() {
                    gdal_sys::GDALSetRasterColorTable(c_tgt_band, c_color_tbl);
                }
            }
            */

            Ok(Dataset::from_c_dataset(c_tgt_ds))
        }
    }

    /// note this is only the number of tgt bands used in the warp op and we might have un-initialized extra bands
    fn get_n_tgt_bands (&self, src_ds: &Dataset) -> Result<c_int> {
        if let Some(n_bands) = self.n_tgt_bands {
            if let Some(src_bands) = &self.src_bands {
                if src_bands.len() > n_bands as usize {
                    return Err(OdinGdalError::MiscError("number of input bands exceeds target bands".to_string()))
                }
            }
            Ok(n_bands as c_int)

        } else { // no explicit number of target bands set
            if let Some(src_bands) = &self.src_bands { // we have explicitly specified src (input) bands
                Ok(src_bands.len() as c_int) // band numbers are 1-based
            } else { // no input / target bands specified -> warp all src bands
                Ok(src_ds.raster_count() as c_int)
            }
        }
    } 

    fn chunk_and_warp (&self, tgt_ds: &mut Dataset) -> Result<()> {
        unsafe {
            reset_last_gdal_error();

            let c_src_ds = self.src_ds.c_dataset();
            let c_tgt_ds = tgt_ds.c_dataset();

            let n_bands = self.src_ds.raster_count() as usize;
            if n_bands == 0 {
                gdal_sys::GDALClose(c_tgt_ds);
                return Err(OdinGdalError::MiscError("no raster bands in input".to_string()))
            }

            let c_warp_options = gdal_sys::GDALCreateWarpOptions();
            let warp_options: &mut GDALWarpOptions = c_warp_options.as_mut().ok_or(last_gdal_error())?;
            warp_options.hSrcDS = self.src_ds.c_dataset();
            warp_options.hDstDS = c_tgt_ds;
            warp_options.dfWarpMemoryLimit = 1073741824 as c_double; // 1G

            self.set_bands( warp_options, n_bands)?;
            self.set_no_data_values( warp_options, tgt_ds);

            warp_options.eResampleAlg = self.resample_alg.clone() as c_uint;

            warp_options.pfnProgress = Some(gdal_sys::GDALDummyProgress);

            //--- proj transformers
            let c_gen_transformer_arg= gdal_sys::GDALCreateGenImgProjTransformer(
                self.src_ds.c_dataset(),
                gdal_sys::GDALGetProjectionRef(self.src_ds.c_dataset()),
                c_tgt_ds,
                gdal_sys::GDALGetProjectionRef(c_tgt_ds),
                0, 0.0, 0
            );
            if c_gen_transformer_arg == null_mut() {
                gdal_sys::GDALClose(c_tgt_ds);
                return Err(last_gdal_error())
            }

            let mut c_transformer_arg = c_gen_transformer_arg;
            let mut c_transformer_func: gdal_sys::GDALTransformerFunc = Some(gdal_sys::GDALGenImgProjTransform);

            let mut c_approx_transformer_arg: *mut c_void = null_mut();
            if self.max_error != 0.0 {
                c_approx_transformer_arg = gdal_sys::GDALCreateApproxTransformer(
                    c_transformer_func,
                    c_gen_transformer_arg,
                    self.max_error);
                if c_approx_transformer_arg == null_mut() {
                    gdal_sys::GDALDestroyGenImgProjTransformer(c_gen_transformer_arg);
                    gdal_sys::GDALClose(c_tgt_ds);
                    return Err(last_gdal_error())
                }

                c_transformer_arg = c_approx_transformer_arg;
                c_transformer_func = Some(gdal_sys::GDALApproxTransform);
            }

            warp_options.pTransformerArg = c_transformer_arg;
            warp_options.pfnTransformer = c_transformer_func;

            let c_warp_op = gdal_sys::GDALCreateWarpOperation(c_warp_options);
            if c_warp_op == null_mut() {
                gdal_sys::GDALDestroyGenImgProjTransformer(warp_options.pTransformerArg);
                gdal_sys::GDALDestroyWarpOptions(c_warp_options);
                return Err(last_gdal_error());
            }

            let x_size = gdal_sys::GDALGetRasterXSize(c_tgt_ds);
            let y_size = gdal_sys::GDALGetRasterYSize(c_tgt_ds);

            let res = gdal_sys::GDALChunkAndWarpImage(c_warp_op, 0,0, x_size, y_size);

            gdal_sys::GDALDestroyWarpOperation(c_warp_op);
            gdal_sys::GDALDestroyGenImgProjTransformer(c_gen_transformer_arg);
            if c_approx_transformer_arg != null_mut() {
                gdal_sys::GDALDestroyApproxTransformer(c_approx_transformer_arg);
            }

            if res == gdal_sys::CPLErr::CE_None {
                gdal_sys::GDALFlushCache(c_tgt_ds);
                Ok(())
            } else {
                Err(last_gdal_error())
            }
        }
    }

    fn set_bands (&self, warp_options: &mut GDALWarpOptions, n_tgt_bands: usize)->Result<()> {
        if let Some(src_bands) = &self.src_bands {
            if src_bands.len() > n_tgt_bands {
                return Err(OdinGdalError::MiscError("number of source exceeds target".to_string()))
            }

            if let Some(tgt_bands) = &self.tgt_bands {
                if src_bands.len() != tgt_bands.len() {
                    return Err(OdinGdalError::MiscError("number of source and target bands differ".to_string()))
                }

                unsafe {
                    let c_tgt_bands = gdal_sys::CPLMalloc(std::mem::size_of::<c_int>() * tgt_bands.len()) as *mut c_int;
                    for i in 0..tgt_bands.len() { *(c_tgt_bands.offset(i as isize)) = tgt_bands[i] as c_int }
                    warp_options.panDstBands = c_tgt_bands;
                }
            } else {
                unsafe {
                    // set from src_bands
                    let c_tgt_bands = gdal_sys::CPLMalloc(std::mem::size_of::<c_int>() * src_bands.len()) as *mut c_int;
                    for i in 0..src_bands.len() { *(c_tgt_bands.offset(i as isize)) = (i+1) as c_int }
                    warp_options.panDstBands = c_tgt_bands;
                }
            }

            warp_options.nBandCount = src_bands.len() as c_int; // number of bands to process
            // NOTE this is freed by GDAL
            unsafe {
                let c_src_bands = gdal_sys::CPLMalloc(std::mem::size_of::<c_int>() * src_bands.len()) as *mut c_int;
                for i in 0..src_bands.len() { *(c_src_bands.offset(i as isize)) = src_bands[i] as c_int }
                warp_options.panSrcBands = c_src_bands;
            }

        } else { // no src/dst band specs - process all bands
            warp_options.nBandCount = 0;
            warp_options.panSrcBands = null_mut();  // TODO - check if that now works with warp API
            warp_options.panDstBands = null_mut();
        }

        Ok(())
    }

    fn set_no_data_values (&self, warp_options: &mut GDALWarpOptions, tgt_ds: &mut Dataset) ->Result<()> {

        if let Some(nodatas) = &self.src_nodatas {
            warp_options.padfSrcNoDataReal = self.create_no_datas( nodatas)?;
        }

        /* TODO has no effect
        let n_output_bands = if let Some(tgt_bands) = &self.tgt_bands { tgt_bands.len() } else { n_input_bands };
        warp_options.padfDstNoDataReal = self.create_no_datas( n_output_bands, &self.tgt_nodata)?;
        */

        // NOTE - most GDAL raster drivers (including GTiff) don't support per-band target nodata values

        if let Some(nodatas) = &self.tgt_nodatas {
            for i in 0..nodatas.len() {
                let band_index = if let Some(tgt_bands) = &self.tgt_bands { tgt_bands[i] as usize } else { i+1 };
                let mut band = tgt_ds.rasterband(band_index)?;
                band.set_no_data_value( Some(nodatas[i]))?;
            }
        }
        //warp_options.padfDstNoDataReal = self.create_no_datas(  &self.tgt_nodatas)?;

        Ok(())
    }

    fn create_no_datas (&self, no_datas: &Vec<c_double>)->Result<*mut f64> {
        let n_bands = no_datas.len();
        unsafe {
            let c_no_datas = gdal_sys::CPLMalloc(std::mem::size_of::<c_double>() * n_bands) as *mut c_double;
            for i in 0..n_bands { 
                *(c_no_datas.offset(i as isize)) = no_datas[i] // FIXME - doesn't work
            }
            Ok(c_no_datas)
        }
    }
}


//--- high level warpers

pub fn warp_to_raster_info<P> (src_ds: &Dataset, tgt_path: P, epsg: u32, tgt_ri: &RasterInfo, alg: ResampleAlg, 
                               src_bands: Option<Vec<u32>>, extra_tgt_bands: Option<usize>, data_type: Option<GdalDataType>) -> Result<Dataset> 
    where P: AsRef<Path>
{
    let tgt_srs = SpatialRef::from_epsg(epsg)?;
    let tgt_format = "GTiff";

    let mut warper = SimpleWarpBuilder::new( &src_ds, tgt_path)?;
    warper.set_tgt_srs( &tgt_srs);
    warper.set_tgt_format( tgt_format);
    warper.set_tgt_extent( tgt_ri.left, tgt_ri.bottom, tgt_ri.right, tgt_ri.top); // FIXME - min/max vs. top/bottom not defined
    warper.set_tgt_resolution( tgt_ri.dx, tgt_ri.dy);
    warper.set_resample_alg(alg);

    if let Some(src_bands) = src_bands {
        warper.set_src_bands(src_bands);
    }

    if let Some(extra_tgt_bands) = extra_tgt_bands {
        warper.set_extra_tgt_bands(extra_tgt_bands);
    }

    if let Some(data_type) = data_type {
        warper.set_data_type(data_type);
    }

    warper.exec()
}

pub fn warp_to_rect<P,Q> (src_path: P, tgt_path: Q, epsg: u32, bbox: &GeoRect, tgt_res: Option<f64>) -> Result<Dataset> 
    where P: AsRef<Path>, Q: AsRef<Path>
{
    let src_ds = Dataset::open(src_path)?;
    let tgt_srs = SpatialRef::from_epsg(epsg)?;
    let tgt_format = "GTiff";

    let mut warper = SimpleWarpBuilder::new( &src_ds, tgt_path)?;
    warper.set_tgt_extent_from_rect(bbox);
    warper.set_tgt_srs( &tgt_srs);
    warper.set_tgt_format( tgt_format);

    if let Some(res) = tgt_res {
        warper.set_tgt_resolution(res, res);
    }

    warper.exec()
}

/// warp to WGS84. Note this requires nodata values since the src bbox might not map to a lon/lat bbox (e.g. for a UTM input SRS) 
pub fn warp_to_wgs84<P,Q> (src_path: P, tgt_path: Q, nodatas: Vec<f64>) -> Result<Dataset> 
    where P: AsRef<Path>, Q: AsRef<Path> 
{
    let src_ds = Dataset::open(src_path)?;
    let tgt_srs = SpatialRef::from_epsg(4326)?;
    let tgt_format = "GTiff";

    let mut warper = SimpleWarpBuilder::new( &src_ds, tgt_path)?;
    warper.set_tgt_srs( &tgt_srs);
    warper.set_tgt_format( tgt_format);
    warper.set_tgt_nodatas(nodatas);

    warper.exec()
}

