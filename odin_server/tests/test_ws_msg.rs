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

use odin_server::prelude::*;

define_ws_payload!{ pub Sentinel = 
    pub device_id: String
}



#[test]
fn test_ws_msg()->OdinServerResult<()> {
    let s1 = Sentinel{device_id: "one".into()};
    let s2 = Sentinel{device_id: "two".into()};
    let v = vec![&s1,&s2];
    
    let sentinels = &v;
    let json = WsMsg::json("odin_sentinel/sentinel_service", "sentinels", sentinels)?;
    println!("{json}");

    Ok(())
}