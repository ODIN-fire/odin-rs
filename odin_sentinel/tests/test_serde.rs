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

use odin_sentinel::{Result,DeviceList,SensorList, RecordList, GpsData, SensorRecord, VocData};

// get {host}/devices
#[test]
fn test_device_list()->Result<()> {
    let input = r#"{"data":[{"id":"roo7gd1dldn3","info":"live"}],"count":1,"total":1,"page":1,"pageCount":1}"#;
    let device_list: DeviceList = serde_json::from_str(input)?;
    println!("-- device-list:\n{device_list:#?}");
    Ok(())
}

// get {host}/devices/{device_id}/sensors
#[test]
fn test_sensor_list()->Result<()> {
    let input = r#"
    {"data":[{"no":0,"deviceId":"roo7gd1dldn3","partNo":"Visible Camera","capabilities":["image","cloudcover"]},{"no":1,"deviceId":"roo7gd1dldn3","partNo":"Visible Camera","capabilities":["image","cloudcover"]},{"no":2,"deviceId":"roo7gd1dldn3","partNo":"Infrared Camera","capabilities":["image"]},{"no":3,"deviceId":"roo7gd1dldn3","partNo":"Infrared Camera","capabilities":["image"]},{"no":4,"deviceId":"roo7gd1dldn3","partNo":"Gas Sensor","capabilities":["gas","thermometer"]},{"no":5,"deviceId":"roo7gd1dldn3","partNo":"VOC Sensor","capabilities":["voc"]},{"no":6,"deviceId":"roo7gd1dldn3","partNo":"9-axis MotionTracking Device","capabilities":["accelerometer","gyroscope","magnetometer"]},{"no":7,"deviceId":"roo7gd1dldn3","partNo":"","capabilities":["fire","smoke"]},{"no":8,"deviceId":"roo7gd1dldn3","partNo":"Anemometer","capabilities":["anemometer"]},{"no":9,"deviceId":"roo7gd1dldn3","partNo":"GPS","capabilities":["gps"]},{"no":10,"deviceId":"roo7gd1dldn3","partNo":"9-axis MotionTracking Device","capabilities":["accelerometer","gyroscope","magnetometer"]},{"no":11,"deviceId":"roo7gd1dldn3","partNo":"9-axis MotionTracking Device","capabilities":["accelerometer","gyroscope","magnetometer","fire","smoke"]},{"no":12,"deviceId":"roo7gd1dldn3","partNo":"","capabilities":["image"]},{"no":13,"deviceId":"roo7gd1dldn3","partNo":"","capabilities":["image","gps"]},{"no":14,"deviceId":"roo7gd1dldn3","partNo":"","capabilities":["gps"]},{"no":15,"deviceId":"roo7gd1dldn3","partNo":"VOC Sensor","capabilities":["voc"]},{"no":16,"deviceId":"roo7gd1dldn3","partNo":"9-axis MotionTracking Device","capabilities":["accelerometer","gyroscope","magnetometer"]},{"no":17,"deviceId":"roo7gd1dldn3","partNo":"9-axis MotionTracking Device","capabilities":["accelerometer","gyroscope","magnetometer"]},{"no":18,"deviceId":"roo7gd1dldn3","partNo":"9-axis MotionTracking Device","capabilities":["accelerometer","gyroscope","magnetometer"]},{"no":19,"deviceId":"roo7gd1dldn3","partNo":null,"capabilities":["image"]},{"no":20,"deviceId":"roo7gd1dldn3","partNo":"test video","capabilities":["image"]},{"no":21,"deviceId":"roo7gd1dldn3","partNo":"Visible Camera","capabilities":["image"]},{"no":22,"deviceId":"roo7gd1dldn3","partNo":"Visible Camera","capabilities":["image"]},{"no":23,"deviceId":"roo7gd1dldn3","partNo":"Infrared Camera","capabilities":["image"]},{"no":24,"deviceId":"roo7gd1dldn3","partNo":"Infrared Camera","capabilities":["image"]},{"no":25,"deviceId":"roo7gd1dldn3","partNo":"Gas Sensor","capabilities":["gas","thermometer","voc"]},{"no":26,"deviceId":"roo7gd1dldn3","partNo":"Anemometer","capabilities":["anemometer"]},{"no":27,"deviceId":"roo7gd1dldn3","partNo":"GPS","capabilities":["gps"]},{"no":28,"deviceId":"roo7gd1dldn3","partNo":"9-axis MotionTracking Device","capabilities":["accelerometer","gyroscope","magnetometer"]},{"no":29,"deviceId":"roo7gd1dldn3","partNo":"9-axis MotionTracking Device","capabilities":["accelerometer","gyroscope","magnetometer"]},{"no":30,"deviceId":"roo7gd1dldn3","partNo":"9-axis MotionTracking Device","capabilities":["accelerometer","gyroscope","magnetometer"]},{"no":31,"deviceId":"roo7gd1dldn3","partNo":"Gas Sensor","capabilities":["gas","thermometer"]},{"no":32,"deviceId":"roo7gd1dldn3","partNo":"AI Detection Output","capabilities":["fire","smoke"]},{"no":33,"deviceId":"roo7gd1dldn3","partNo":"test video","capabilities":["image"]},{"no":34,"deviceId":"roo7gd1dldn3","partNo":"Orientation","capabilities":["orientation"]},{"no":35,"deviceId":"roo7gd1dldn3","partNo":"Orientation","capabilities":["orientation"]},{"no":36,"deviceId":"roo7gd1dldn3","partNo":"Infrared Camera","capabilities":["image"]},{"no":37,"deviceId":"roo7gd1dldn3","partNo":"Infrared Camera","capabilities":["image"]},{"no":38,"deviceId":"roo7gd1dldn3","partNo":"Gas Sensor","capabilities":["gas","thermometer"]},{"no":39,"deviceId":"roo7gd1dldn3","partNo":"VOC Sensor","capabilities":["voc"]},{"no":40,"deviceId":"roo7gd1dldn3","partNo":"Anemometer","capabilities":["anemometer"]},{"no":41,"deviceId":"roo7gd1dldn3","partNo":"GPS","capabilities":["gps"]},{"no":42,"deviceId":"roo7gd1dldn3","partNo":"AI Detection Output","capabilities":["fire","smoke"]},{"no":43,"deviceId":"roo7gd1dldn3","partNo":"Test Video","capabilities":["image"]},{"no":44,"deviceId":"roo7gd1dldn3","partNo":"Orientation","capabilities":[]}],"count":45,"total":45,"page":1,"pageCount":1}
    "#;
    let sensor_list: SensorList = serde_json::from_str(input)?;
    println!("-- sensor-list:\n{sensor_list:#?}");
    Ok(())
}

//{host}/devices/{device_id}/sensors/{sensor_no}/{capability}?sort=timeRecorded,DESC&limit={n_last}
#[test]
fn test_sensor_gps_records()->Result<()> {
    let input = r#"
    {"data":[{"id":"crmWhFT3LMHdItHFTUGi","type":"gps","timeRecorded":"2023-01-29T19:32:04.000Z","sensorNo":9,"deviceId":"roo7gd1dldn3","gps":{"latitude":34.16381345,"longitude":-118.10208433333334,"altitude":null,"quality":null,"numberOfSatellites":null,"HDOP":null},"evidences":[],"claims":[]},{"id":"Za1Y9LIYQ7KXSNbeDNBb","type":"gps","timeRecorded":"2023-01-29T19:31:34.000Z","sensorNo":9,"deviceId":"roo7gd1dldn3","gps":{"latitude":34.163813383333334,"longitude":-118.10208601666666,"altitude":null,"quality":null,"numberOfSatellites":null,"HDOP":null},"evidences":[],"claims":[]},{"id":"rUEGekTnRjD7opkqxJAw","type":"gps","timeRecorded":"2023-01-29T19:31:03.000Z","sensorNo":9,"deviceId":"roo7gd1dldn3","gps":{"latitude":34.16381325,"longitude":-118.10208675,"altitude":null,"quality":null,"numberOfSatellites":null,"HDOP":null},"evidences":[],"claims":[]}],"count":3,"total":156061,"page":1,"pageCount":52021}
    "#;

    let gps_record_list: RecordList<GpsData> = serde_json::from_str(input)?;
    println!("-- GPS record-list:\n{gps_record_list:#?}");

    Ok(())
}

#[test]
fn test_serde_roundtrip()->Result<()> {
    // since we derive Deserialize but impl Serialize we have to test the roundtrip
    let input = r#"{"id":"eYdrMhE4b55MO87oJF9r","timeRecorded":"2024-01-23T20:32:01.004Z","sensorNo":39,"deviceId":"roo7gd1dldn3","evidences":[],"claims":[],"voc":{"TVOC":138,"eCO2":489}}"#;

    let rec: SensorRecord<VocData> = serde_json::from_str(input)?;
    println!("parsed voc record: {:?}", rec);

    // The deserialization of the DateTime<Utc> is from a date string, while it is serialized as
    // epic millis (i64). The input to Odin comes as a string, but the javascript wants the i64.
    let json = serde_json::to_string(&rec)?
        .replace("1706041921004", r#""2024-01-23T20:32:01.004Z""#);
    println!("generated json: {}", json);
    assert_eq!( json.as_str(), input);
    Ok(())
}
