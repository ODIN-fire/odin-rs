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

use odin_orbital::tle_store::{parse_tle_lines, TLE_LINES_RE};

/* #region test-data *************************************************************/

const INPUT: &'static str = r#"[
  {
    "CCSDS_OMM_VERS": "2.0",
    "COMMENT": "GENERATED VIA SPACE-TRACK.ORG API",
    "CREATION_DATE": "2025-03-18T02:25:22",
    "ORIGINATOR": "18 SPCS",
    "OBJECT_NAME": "NOAA 21",
    "OBJECT_ID": "2022-150A",
    "CENTER_NAME": "EARTH",
    "REF_FRAME": "TEME",
    "TIME_SYSTEM": "UTC",
    "MEAN_ELEMENT_THEORY": "SGP4",
    "EPOCH": "2025-03-17T22:16:50.050848",
    "MEAN_MOTION": "14.19556514",
    "ECCENTRICITY": "0.00027100",
    "INCLINATION": "98.7204",
    "RA_OF_ASC_NODE": "17.0432",
    "ARG_OF_PERICENTER": "72.7407",
    "MEAN_ANOMALY": "287.4066",
    "EPHEMERIS_TYPE": "0",
    "CLASSIFICATION_TYPE": "U",
    "NORAD_CAT_ID": "54234",
    "ELEMENT_SET_NO": "999",
    "REV_AT_EPOCH": "12181",
    "BSTAR": "0.00019403000000",
    "MEAN_MOTION_DOT": "0.00000366",
    "MEAN_MOTION_DDOT": "0.0000000000000",
    "SEMIMAJOR_AXIS": "7204.990",
    "PERIOD": "101.440",
    "APOAPSIS": "828.808",
    "PERIAPSIS": "824.902",
    "OBJECT_TYPE": "PAYLOAD",
    "RCS_SIZE": "LARGE",
    "COUNTRY_CODE": "US",
    "LAUNCH_DATE": "2022-11-10",
    "SITE": "AFWTR",
    "DECAY_DATE": null,
    "FILE": "4672421",
    "GP_ID": "283377184",
    "TLE_LINE0": "0 NOAA 21",
    "TLE_LINE1": "1 54234U 22150A   25076.92835707  .00000366  00000-0  19403-3 0  9994",
    "TLE_LINE2": "2 54234  98.7204  17.0432 0002710  72.7407 287.4066 14.19556514121811"
  },
  {
    "CCSDS_OMM_VERS": "2.0",
    "COMMENT": "GENERATED VIA SPACE-TRACK.ORG API",
    "CREATION_DATE": "2025-03-17T19:07:54",
    "ORIGINATOR": "18 SPCS",
    "OBJECT_NAME": "NOAA 21",
    "OBJECT_ID": "2022-150A",
    "CENTER_NAME": "EARTH",
    "REF_FRAME": "TEME",
    "TIME_SYSTEM": "UTC",
    "MEAN_ELEMENT_THEORY": "SGP4",
    "EPOCH": "2025-03-17T13:49:20.880768",
    "MEAN_MOTION": "14.19555996",
    "ECCENTRICITY": "0.00027230",
    "INCLINATION": "98.7204",
    "RA_OF_ASC_NODE": "16.6962",
    "ARG_OF_PERICENTER": "73.3399",
    "MEAN_ANOMALY": "286.8075",
    "EPHEMERIS_TYPE": "0",
    "CLASSIFICATION_TYPE": "U",
    "NORAD_CAT_ID": "54234",
    "ELEMENT_SET_NO": "999",
    "REV_AT_EPOCH": "12176",
    "BSTAR": "0.00017437000000",
    "MEAN_MOTION_DOT": "0.00000324",
    "MEAN_MOTION_DDOT": "0.0000000000000",
    "SEMIMAJOR_AXIS": "7204.992",
    "PERIOD": "101.440",
    "APOAPSIS": "828.819",
    "PERIAPSIS": "824.895",
    "OBJECT_TYPE": "PAYLOAD",
    "RCS_SIZE": "LARGE",
    "COUNTRY_CODE": "US",
    "LAUNCH_DATE": "2022-11-10",
    "SITE": "AFWTR",
    "DECAY_DATE": null,
    "FILE": "4672105",
    "GP_ID": "283341078",
    "TLE_LINE0": "0 NOAA 21",
    "TLE_LINE1": "1 54234U 22150A   25076.57593612  .00000324  00000-0  17437-3 0  9990",
    "TLE_LINE2": "2 54234  98.7204  16.6962 0002723  73.3399 286.8075 14.19555996121765"
  }
]"#;

/* #endregion test-data */

#[test]
fn test_parse_lines() {
    let tle_lines = parse_tle_lines(INPUT);
    
    for tle in tle_lines {
        println!("{}", tle.0);
        println!("{}", tle.1);
        println!("{}", tle.2);
    }

    assert!(tle.lines.len() == 2)
}