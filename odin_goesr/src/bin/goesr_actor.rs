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
use anyhow::Result;
use std::time::Duration;
use std::path::PathBuf;
use odin_actor::prelude::*;
use odin_goesr::actor::GoesRImportActor;
use odin_goesr::{GoesRImportActorConfig, LiveGoesRDataImporter, GoesRProduct};

#[tokio::main]
async fn main() -> Result<()>{
    let goesr_product:GoesRProduct = GoesRProduct {
      name: String::from("ABI-L2-FDCC"),
      bucket: String::from("noaa-goes18"),
      history: String::from("1d")
    };
    
    let config:GoesRImportActorConfig = GoesRImportActorConfig{
      polling_interval: Duration::from_secs(60*5),
      satellite: 18,
      data_dir: PathBuf::from("..\\..\\race-data\\goesr_test"),
      keep_files: true,
      s3_region: String::from("us-east-1"),
      products: vec![goesr_product],
      init_records: 3,
      max_records:10
    };
    
    let mut actor_system = ActorSystem::new("main");

    let _actor_handle = spawn_actor!( actor_system, "goesr",  GoesRImportActor::new(config.clone(), LiveGoesRDataImporter::new(config.clone())).await)?;
    actor_system.start_all().await?;
    let _ = actor_system.process_requests().await;

    Ok(())
}