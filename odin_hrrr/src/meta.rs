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

 // TODO - the GRIB_IDS entry should be computed to reflect the right date

/// grib2 specific band meta info for HRRR fields
/// format is &[ (domain: &str, key: &str, value: &str) ]

pub const TMP_2_HTGL_GRIB_META: &[(&str,&str,&str)] = &[
    ("Description", "2[m] HTGL=\"Specified height level above ground\"", ""),
    ("GRIB_UNIT","[C]", ""),
    ("GRIB_COMMENT","Temperature [C]", ""),
    ("GRIB_ELEMENT","TMP", ""),
    ("GRIB_SHORT_NAME","2-HTGL", ""),
    ("GRIB_DISCIPLINE","0(Meteorological)", ""),
    ("GRIB_IDS=CENTER", "7(US-NCEP) SUBCENTER=0 MASTER_TABLE=2 LOCAL_TABLE=1 SIGNF_REF_TIME=1(Start_of_Forecast) REF_TIME=2026-02-26T23:00:00Z PROD_STATUS=0(Operational) TYPE=1(Forecast)" ,""),
    ("GRIB_PDS_PDTN","0", ""),
    ("GRIB_PDS_TEMPLATE_NUMBERS","0 0 2 0 83 0 0 0 1 0 0 0 0 103 0 0 0 0 2 255 0 0 0 0 0", ""),
    ("GRIB_PDS_TEMPLATE_ASSEMBLED_VALUES","0 0 2 0 83 0 0 1 0 103 0 2 255 0 0", "")
];

pub const TCDC_0_EATM_GRIB_META: &[(&str,&str,&str)] = &[
    ("Description", "0[-] EATM=\"Entire Atmosphere\"", ""),
    ("GRIB_UNIT","[%]", ""),
    ("GRIB_COMMENT","Total cloud cover [%]", ""),
    ("GRIB_ELEMENT","TCDC", ""),
    ("GRIB_SHORT_NAME","0-EATM", ""),
    ("GRIB_DISCIPLINE","0(Meteorological)", ""),
    ("GRIB_IDS=CENTER", "7(US-NCEP) SUBCENTER=0 MASTER_TABLE=2 LOCAL_TABLE=1 SIGNF_REF_TIME=1(Start_of_Forecast) REF_TIME=2026-02-26T23:00:00Z PROD_STATUS=0(Operational) TYPE=1(Forecast)",""),
    ("GRIB_PDS_PDTN","0", ""),
    ("GRIB_PDS_TEMPLATE_NUMBERS","6 1 2 0 83 0 0 0 1 0 0 0 0 10 0 0 0 0 0 255 0 0 0 0 0", ""),
    ("GRIB_PDS_TEMPLATE_ASSEMBLED_VALUES","6 1 2 0 83 0 0 1 0 10 0 0 255 0 0", "")
];

pub const UGRD_10_HTGL_GRIB_META: &[(&str,&str,&str)] = &[
    ("Description", "10[m] HTGL=\"Specified height level above ground\"", ""),
    ("GRIB_UNIT","[m/s]", ""),
    ("GRIB_COMMENT","u-component of wind [m/s]", ""),
    ("GRIB_ELEMENT","UGRD", ""),
    ("GRIB_SHORT_NAME","10-HTGL", ""),
    ("GRIB_DISCIPLINE","0(Meteorological)", ""),
    ("GRIB_IDS=CENTER", "7(US-NCEP) SUBCENTER=0 MASTER_TABLE=2 LOCAL_TABLE=1 SIGNF_REF_TIME=1(Start_of_Forecast) REF_TIME=2026-02-26T23:00:00Z PROD_STATUS=0(Operational) TYPE=1(Forecast)",""),
    ("GRIB_PDS_PDTN","0", ""),
    ("GRIB_PDS_TEMPLATE_NUMBERS","2 2 2 0 83 0 0 0 1 0 0 0 0 103 0 0 0 0 10 255 0 0 0 0 0", ""),
    ("GRIB_PDS_TEMPLATE_ASSEMBLED_VALUES","2 2 2 0 83 0 0 1 0 103 0 10 255 0 0", "")
];

pub const VGRD_10_HTGL_GRIB_META: &[(&str,&str,&str)] = &[
    ("Description", "10[m] HTGL=\"Specified height level above ground\"", ""),
    ("GRIB_UNIT","[m/s]", ""),
    ("GRIB_COMMENT","v-component of wind [m/s]", ""),
    ("GRIB_ELEMENT","VGRD", ""),
    ("GRIB_SHORT_NAME","10-HTGL", ""),
    ("GRIB_DISCIPLINE","0(Meteorological)", ""),
    ("GRIB_IDS=CENTER", "7(US-NCEP) SUBCENTER=0 MASTER_TABLE=2 LOCAL_TABLE=1 SIGNF_REF_TIME=1(Start_of_Forecast) REF_TIME=2026-02-26T23:00:00Z PROD_STATUS=0(Operational) TYPE=1(Forecast)",""),
    ("GRIB_PDS_PDTN","0", ""),
    ("GRIB_PDS_TEMPLATE_NUMBERS","2 3 2 0 83 0 0 0 1 0 0 0 0 103 0 0 0 0 10 255 0 0 0 0 0", ""),
    ("GRIB_PDS_TEMPLATE_ASSEMBLED_VALUES","2 3 2 0 83 0 0 1 0 103 0 10 255 0 0", "")
];

pub const UGRD_80_HTGL_GRIB_META: &[(&str,&str,&str)] = &[
    ("Description", "80[m] HTGL=\"Specified height level above ground\"", ""),
    ("GRIB_UNIT","[m/s]", ""),
    ("GRIB_COMMENT","u-component of wind [m/s]", ""),
    ("GRIB_ELEMENT","UGRD", ""),
    ("GRIB_SHORT_NAME","80-HTGL", ""),
    ("GRIB_DISCIPLINE","0(Meteorological)", ""),
    ("GRIB_IDS=CENTER", "7(US-NCEP) SUBCENTER=0 MASTER_TABLE=2 LOCAL_TABLE=1 SIGNF_REF_TIME=1(Start_of_Forecast) REF_TIME=2026-02-26T23:00:00Z PROD_STATUS=0(Operational) TYPE=1(Forecast)", ""),
    ("GRIB_PDS_PDTN","0", ""),
    ("GRIB_PDS_TEMPLATE_NUMBERS","2 2 2 0 83 0 0 0 1 0 0 0 0 103 0 0 0 0 80 255 0 0 0 0 0", ""),
    ("GRIB_PDS_TEMPLATE_ASSEMBLED_VALUES","2 2 2 0 83 0 0 1 0 103 0 80 255 0 0", "")
];

pub const VGRD_80_HTGL_GRIB_META: &[(&str,&str,&str)] = &[
    ("Description", "80[m] HTGL=\"Specified height level above ground\"", ""),
    ("GRIB_UNIT","[m/s]", ""),
    ("GRIB_COMMENT","v-component of wind [m/s]", ""),
    ("GRIB_ELEMENT","VGRD", ""),
    ("GRIB_SHORT_NAME","80-HTGL", ""),
    ("GRIB_DISCIPLINE","0(Meteorological)", ""),
    ("GRIB_IDS=CENTER", "7(US-NCEP) SUBCENTER=0 MASTER_TABLE=2 LOCAL_TABLE=1 SIGNF_REF_TIME=1(Start_of_Forecast) REF_TIME=2026-02-26T23:00:00Z PROD_STATUS=0(Operational) TYPE=1(Forecast)",""),
    ("GRIB_PDS_PDTN","0", ""),
    ("GRIB_PDS_TEMPLATE_NUMBERS","2 3 2 0 83 0 0 0 1 0 0 0 0 103 0 0 0 0 80 255 0 0 0 0 0", ""),
    ("GRIB_PDS_TEMPLATE_ASSEMBLED_VALUES","2 3 2 0 83 0 0 1 0 103 0 80 255 0 0", "")
];

// TODO - HRRR only has 80m - shall we keep this for other models?
pub const UGRD_100_HTGL_GRIB_META: &[(&str,&str,&str)] = &[
    ("Description", "100[m] HTGL=\"Specified height level above ground\"", ""),
    ("GRIB_UNIT","[m/s]", ""),
    ("GRIB_COMMENT","u-component of wind [m/s]", ""),
    ("GRIB_ELEMENT","UGRD", ""),
    ("GRIB_SHORT_NAME","100-HTGL", ""),
    ("GRIB_DISCIPLINE","0(Meteorological)", ""),
    ("GRIB_PDS_PDTN","0", ""),
    ("GRIB_PDS_TEMPLATE_NUMBERS","2 2 2 0 83 0 0 0 1 0 0 0 0 103 0 0 0 0 100 255 0 0 0 0 0", ""),
    ("GRIB_PDS_TEMPLATE_ASSEMBLED_VALUES","2 2 2 0 83 0 0 1 0 103 0 100 255 0 0", "")
];

pub const VGRD_100_HTGL_GRIB_META: &[(&str,&str,&str)] = &[
    ("Description", "100[m] HTGL=\"Specified height level above ground\"", ""),
    ("GRIB_UNIT","[m/s]", ""),
    ("GRIB_COMMENT","v-component of wind [m/s]", ""),
    ("GRIB_ELEMENT","VGRD", ""),
    ("GRIB_SHORT_NAME","100-HTGL", ""),
    ("GRIB_DISCIPLINE","0(Meteorological)", ""),
    ("GRIB_PDS_PDTN","0", ""),
    ("GRIB_PDS_TEMPLATE_NUMBERS","2 3 2 0 83 0 0 0 1 0 0 0 0 103 0 0 0 0 100 255 0 0 0 0 0", ""),
    ("GRIB_PDS_TEMPLATE_ASSEMBLED_VALUES","2 3 2 0 83 0 0 1 0 103 0 100 255 0 0", "")
];

pub const RH_2_HTGL_GRIB_META: &[(&str,&str,&str)] = &[
    ("Description", "2[m] HTGL=\"Specified height level above ground\"", ""),
    ("GRIB_UNIT", "[%]", ""),
    ("GRIB_COMMENT", "Relative humidity [%]", ""),
    ("GRIB_ELEMENT", "RH", ""),
    ("GRIB_SHORT_NAME", "2-HTGL", ""),
    ("GRIB_DISCIPLINE", "0(Meteorological)", ""),
    ("GRIB_IDS=CENTER", "7(US-NCEP) SUBCENTER=0 MASTER_TABLE=2 LOCAL_TABLE=1 SIGNF_REF_TIME=1(Start_of_Forecast) REF_TIME=2026-02-26T23:00:00Z PROD_STATUS=0(Operational) TYPE=1(Forecast)",""),
    ("GRIB_PDS_PDTN", "0", ""),
    ("GRIB_PDS_TEMPLATE_NUMBERS", "1 1 2 0 83 0 0 0 1 0 0 0 0 103 0 0 0 0 2 255 0 0 0 0 0", ""),
    ("GRIB_PDS_TEMPLATE_ASSEMBLED_VALUES", "1 1 2 0 83 0 0 1 0 103 0 2 255 0 0", "")
];

pub const PRES_0_SFC_GRIB_META: &[(&str,&str,&str)] = &[
    ("Description", "0[-] SFC=\"Ground or water surface\"", ""),
    ("GRIB_UNIT", "[Pa]", ""),
    ("GRIB_COMMENT", "Pressure [Pa]", ""),
    ("GRIB_ELEMENT", "PRES", ""),
    ("GRIB_SHORT_NAME", "0-SFC", ""),
    ("GRIB_DISCIPLINE", "0(Meteorological)", ""),
    ("GRIB_IDS=CENTER", "7(US-NCEP) SUBCENTER=0 MASTER_TABLE=2 LOCAL_TABLE=1 SIGNF_REF_TIME=1(Start_of_Forecast) REF_TIME=2026-02-26T23:00:00Z PROD_STATUS=0(Operational) TYPE=1(Forecast)",""),
    ("GRIB_PDS_PDTN", "0", ""),
    ("GRIB_PDS_TEMPLATE_NUMBERS", "3 0 2 0 83 0 0 0 1 0 0 0 0 1 0 0 0 0 0 255 0 0 0 0 0", ""),
    ("GRIB_PDS_TEMPLATE_ASSEMBLED_VALUES", "3 0 2 0 83 0 0 1 0 1 0 0 255 0 0", "")
];

pub const HGT_0_SFC_GRIB_META: &[(&str,&str,&str)] = &[
    ("Description", " 0[-] SFC=\"Ground or water surface\"", ""),
    ("GRIB_UNIT","[gpm]", ""),
    ("GRIB_COMMENT","Geopotential height [gpm]", ""),
    ("GRIB_ELEMENT","HGT", ""),
    ("GRIB_SHORT_NAME","0-SFC", ""),
    ("GRIB_DISCIPLINE","0(Meteorological)",""),
    ("GRIB_PDS_PDTN","0",""),
    ("GRIB_PDS_TEMPLATE_NUMBERS","3 5 2 0 83 0 0 0 1 0 0 0 0 1 0 0 0 0 0 255 0 0 0 0 0", ""),
    ("GRIB_PDS_TEMPLATE_ASSEMBLED_VALUES","3 5 2 0 83 0 0 1 0 1 0 0 255 0 0","")
];
