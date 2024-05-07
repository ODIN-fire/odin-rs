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

use tokio;
use std::time::Duration;
use odin_action::*;

use anyhow::anyhow;

#[tokio::main]
async fn main() {
    let x = "gna1".to_string(); // captured context

    let action = data_action!( x: String => |data: String| { 
        println!("DataAction<String> execution started ...");
        map_action_err( fumble())?;
        //fumble().map_err(|e| OdinActionError::from(e))?;
        tokio::time::sleep( Duration::from_secs(2)).await;
        println!("action called with data={data} and x={x}");
        action_ok()
    });

    //da.execute("DA".to_string()).await;
    exec_action( action, "DA".to_string()).await;
}

fn fumble ()->anyhow::Result<()> {
    Err(anyhow!("I'm fumbled"))
}

async fn exec_action <T> (da: impl DataAction<T>, data: T) {
    println!("now executing {da:?}");
    da.execute(data).await.expect("action returned error");
}