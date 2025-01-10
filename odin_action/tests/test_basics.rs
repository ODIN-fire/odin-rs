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

use odin_action::*;
use std::time::Duration;
use tokio;

#[tokio::test]
async fn test_da() {
    let da = data_action!( let x: String = "gna1".to_string() => |data: String| {
        println!("DataAction<String> execution started ...");
        tokio::time::sleep( Duration::from_secs(2)).await;
        println!("action called with data={data} and x={x}");
        Ok(())
    });
    //da.execute("DA".to_string()).await;
    exec_da( da, "DA".to_string()).await;
}

async fn exec_da <T> (da: impl DataAction<T>, data: T) {
    println!();
    println!("now executing {da:?}");
    da.execute(data).await.expect("action returned error");
}

#[tokio::test]
async fn test_dda() {
    let dda: DynDataAction<String> = dyn_data_action!( let x: String = "gna2".to_string() => |data: String| {
        println!("DynDataAction<String> execution started ...");
        tokio::time::sleep( Duration::from_secs(2)).await;
        println!("action called with data={data} and x={x}");
        Ok(())
    });
    //dda.execute("DDA".to_string()).await;
    exec_dda( dda, "DDA".to_string()).await;
}

async fn exec_dda<T> (dda: DynDataAction<T>, data: T) {
    println!();
    println!("now executing {dda:?}");
    dda.execute(data).await.expect("action returned error");
}

#[tokio::test]
async fn test_dra() {
    let dra = dataref_action!( let x: String = "gna3".to_string() => |data: &String| {
        println!("DataRefAction<String> execution started ...");
        tokio::time::sleep( Duration::from_secs(2)).await;
        println!("action called with data={data} and x={x}");
        Ok(())
    });
    let data = "DRA".to_string();
    //dra.execute(&data).await;
    exec_dra( dra, &data).await;
}

async fn exec_dra <T> (dra: impl DataRefAction<T>, data: &T) {
    println!();
    println!("now executing {dra:?}");
    dra.execute(data).await.expect("action returned error");
}

#[tokio::test]
async fn test_bdra() {
    let bdra = bi_dataref_action!( let x: String = "gna4".to_string() => |data: &String, bidata: i64| {
        println!("BiDataRefAction<String,i64> execution started ...");
        tokio::time::sleep( Duration::from_secs(2)).await;
        println!("action called with data={data}, bidata={bidata} and x={x}");
        Ok(())
    });
    let data = "BDRA".to_string();
    //bdra.execute(&data).await;
    exec_bdra( bdra, &data, 42).await;

    let bdra = no_bi_dataref_action();
    exec_bdra( bdra, &data, 42).await;
}

async fn exec_bdra <T,A> (adra: impl BiDataRefAction<T,A>, data: &T, bidata: A) {
    println!();
    println!("now executing {adra:?}");
    adra.execute( data, bidata).await.expect("action returned error");
}


#[tokio::test]
async fn test_ddra() {
    let ddra: DynDataRefAction<String> = dyn_dataref_action!( let x: String = "gna5".to_string() => |data: &String| {
        println!("DynDataRefAction<String> execution started ...");
        tokio::time::sleep( Duration::from_secs(2)).await;
        println!("action called with data={data} and x={x}");
        Ok(())
    });
    let data = "DDRA".to_string();
    //ddra.execute(&data).await;
    exec_ddra( ddra, &data).await;
}

async fn exec_ddra<T> (ddra: DynDataRefAction<T>, data: &T) {
    println!();
    println!("now executing {ddra:?}");
    ddra.execute(data).await.expect("action returned error");
}
