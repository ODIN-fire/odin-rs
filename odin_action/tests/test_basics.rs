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

//--- tests for the unified by-value/reference actions

#[derive(Debug)]
struct Store { v: Vec<u64> }

/// an actor-like owner that executes an action with a reference to its own data in a spawned (Send) task
struct Owner<A> where A: DataRefAction<Store> { store: Store, action: A }

impl<A> Owner<A> where A: DataRefAction<Store> + Sync + 'static {
    async fn run (&self)->Result<(),OdinActionFailure> {
        tokio::task::yield_now().await;
        self.action.execute( &self.store).await?; // data borrowed across await points
        tokio::task::yield_now().await;
        Ok(())
    }
}

#[tokio::test]
async fn test_ref_action_in_task () {
    let a = data_action!( let tag: String = "da".to_string() => |s: &Store| { println!("{tag}: {:?}", s.v); Ok(()) });
    let owner = std::sync::Arc::new( Owner{ store: Store{ v: vec![1,2,3] }, action: a });
    let o = owner.clone();
    tokio::spawn( async move { o.run().await.unwrap() }).await.unwrap();

    // the empty action satisfies the reference bounds too
    let o = Owner{ store: Store{ v: vec![] }, action: no_dataref_action() };
    assert!( o.action.is_empty());
    o.run().await.unwrap();

    // explicit higher ranked bound is equivalent to DataRefAction
    async fn exec_hr<A: for<'a> DataAction<&'a Store>> (a: &A, s: &Store) { a.execute(s).await.unwrap() }
    exec_hr( &owner.action, &owner.store).await;
}

#[tokio::test]
async fn test_mut_actions () {
    let mut da = data_mut_action!( let mut count: usize = 0 => |data: u64| {
        *count += 1;
        println!("DataMutAction<u64> call {count} with {data}");
        Ok(())
    });
    da.execute( 1).await.unwrap();
    da.execute( 2).await.unwrap();

    let mut dra = dataref_mut_action!( let mut sum: u64 = 0 => |data: &Store| {
        *sum += data.v.iter().sum::<u64>();
        println!("DataRefMutAction<Store> sum = {sum}");
        if *sum > 100 { Err( OdinActionFailure("sum too large".into())) } else { Ok(()) }
    });
    exec_drma( &mut dra, &Store{ v: vec![10,20] }).await.unwrap();
    assert!( exec_drma( &mut dra, &Store{ v: vec![100] }).await.is_err());
}

async fn exec_drma <A: DataRefMutAction<Store>> (a: &mut A, data: &Store)->Result<(),OdinActionFailure> {
    a.execute( data).await
}

#[tokio::test]
async fn test_bi_data_action () {
    let bda = bi_data_action!( let x: String = "gna6".to_string() => |data: u64, bidata: String| {
        println!("BiDataAction<u64,String> called with data={data}, bidata={bidata} and x={x}");
        let n: u64 = bidata.parse()?; // error conversion into OdinActionFailure
        assert_eq!( n, data);
        Ok(())
    });
    bda.execute( 42, "42".to_string()).await.unwrap();
    assert!( bda.execute( 42, "not a number".to_string()).await.is_err());

    exec_bdra( no_bi_dataref_action(), &"none".to_string(), 0).await;
}

#[tokio::test]
async fn test_dyn_ref_actions () {
    let mut list: DynDataRefActionList<Store> = DynDataRefActionList::new();
    for i in 0..3 {
        list.push( dyn_dataref_action!( let i: usize = i => |s: &Store| {
            tokio::task::yield_now().await;
            println!("DynDataRefAction {i}: {:?}", s.v);
            Ok(())
        }));
    }
    let store = Store{ v: vec![7,8] };
    tokio::spawn( async move { list.execute( &store, false).await.unwrap() }).await.unwrap(); // in Send task

    // dyn_data_action! with a reference argument is the same type
    let d: DynDataRefAction<Store> = dyn_data_action!( => |s: &Store| { println!("dyn: {}", s.v.len()); Ok(()) });
    d.execute( &Store{ v: vec![1] }).await.unwrap();

    // by-value dyn actions with error conversion
    let d: DynDataAction<String> = dyn_data_action!( => |s: String| { let n: i32 = s.parse()?; println!("parsed {n}"); Ok(()) });
    assert!( d.execute( "x".to_string()).await.is_err());
}
