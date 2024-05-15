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
use crate::*;

 #[derive(Debug)]
pub struct LiveGoesRDataImporter {
    pub config: GoesRImportActorConfig,
    pub data_dir: Arc<PathBuf>,
    pub file_cleanup_task: Option<AbortHandle>,
    pub import_task: Option<AbortHandle>
}

impl LiveGoesRDataImporter {
    pub fn new (config: GoesRImportActorConfig) -> Self {
        LiveGoesRDataImporter {
            data_dir: Arc::new( odin_config::app_metadata().data_dir.join("goesr")),
            config: config,
            file_cleanup_task: None,
            import_task: None
        }
    }

    async fn initialize  (&mut self, hself: ActorHandle<GoesRActorMsg>) -> Result<()> { 
        self.run_import_task(hself).await?;
        self.run_file_cleanup_task()?;
        Ok(())
    }

    async fn file_cleanup_loop (config: GoesRImportActorConfig)->Result<()> {
        let interval = minutes(60);
        let data_dir = odin_config::app_metadata().data_dir.join("goesr");
        loop {
            sleep(interval).await;
            remove_old_files( &data_dir, config.max_age);
        }
    }

    async fn run_import_task(&mut self, hself: ActorHandle<GoesRActorMsg>) -> Result<()> { 
        let mut task = LiveGoesRDataAcquisitionTask::new(self.config.clone(), hself).await?;
        let import_task = spawn( "goesr-data-acquisition", async move {
            let _ = task.spawn_data_acquitision_task().await;
            }
        )?.abort_handle();
        self.import_task = Some(import_task);
        Ok(())
    }

    fn run_file_cleanup_task(&mut self)-> Result<()> {
        let file_cleanup_task = spawn("goesr-file-cleanup", Self::file_cleanup_loop( self.config.clone()))?.abort_handle();
        self.file_cleanup_task = Some(file_cleanup_task);
        Ok(())
    }
}

impl GoesRDataImporter for LiveGoesRDataImporter {
    async fn start (&mut self, hself: ActorHandle<GoesRActorMsg>) -> Result<()> {
        self.initialize(hself).await?;
        Ok(())
    }
    fn terminate (&mut self) {
        if let Some(task) = &self.import_task {
            task.abort();
        }
        if let Some(task) = &self.file_cleanup_task {
            task.abort();
        }
    }
}

#[derive(Debug)]
pub struct LiveGoesRDataAcquisitionTask { 
    pub latest_objs: HashMap<String, Object>,
    pub sat_id: u8,
    pub polling_interval: Duration,
    pub s3_client:Client,
    pub product: GoesRProduct,
    pub data_dir: PathBuf, // obtain from config
    pub init_records:usize,
    pub hself: ActorHandle<GoesRActorMsg>

}

impl LiveGoesRDataAcquisitionTask {
    // add download task, file cleanup task
    pub async fn new(config:GoesRImportActorConfig, hself:ActorHandle<GoesRActorMsg>) -> Result<Self> {
        let region_provider = RegionProviderChain::first_try(Region::new(config.s3_region.clone()));
        let aws_config = aws_config::from_env().no_credentials().region(region_provider).load().await; // add anonymous creditials
        let s3_client = Client::new(&aws_config);
        let latest_objs:HashMap<String, Object> = HashMap::new();
        let live_task = LiveGoesRDataAcquisitionTask {
            latest_objs: latest_objs,
            sat_id: config.satellite,
            polling_interval: config.polling_interval,
            s3_client: s3_client,
            product: config.product,
            data_dir:   odin_config::app_metadata().data_dir.join(format!("goesr-{}", config.satellite)),
            init_records: config.init_records,
            hself: hself
        };
        Ok(live_task)
    }

    pub async fn initial_download(&mut self) -> Result<Vec<GoesRHotSpots>> {
        //downloads x amount of files
        // updates latest obj
        let product = &self.product;
        let dt = Utc::now();
        let num_obj=self.init_records;
        let init_objs = get_inital_objects(&self.s3_client, dt, product, &self.sat_id,  num_obj).await?;
        if init_objs.len() > 0 {
            let most_recent = get_most_recent_obj_from_vec(&init_objs)?;
            self.latest_objs.insert(product.name.clone(), most_recent.clone());
            let data = join_all(init_objs.iter().map(|x| async{get_goesr_data(&self.s3_client, x.clone(), &self.data_dir, product, self.sat_id.clone()).await})).await;
            let goesr_data: Result<Vec<GoesRData>> = data.into_iter().collect();
            let goesr_data_vec = goesr_data?;
            let hotspots_res:  Result<Vec<GoesRHotSpots>>  = goesr_data_vec.into_iter().map( |x| read_goesr_data(&x)).into_iter().collect();
            let hotspots = hotspots_res?;
            Ok(hotspots)
        } else {
            Err(OdinGoesRError::NoObjectError(String::from("No objects for GOES-R product and datetime initialization")))
        }
    }
    
    pub async fn download_updates(&mut self) -> Result<GoesRHotSpots> {
        //downloads latest file
        let product = &self.product;
        let last_object = if let Some(l_obj) = self.latest_objs.get(&product.name) {
            Some(l_obj)
        } else { 
            None
        };
        let dt = Utc::now();
        let prefix = format!("{}/{}/{:03}/{:02}/", product.name, dt.year(), dt.ordinal(), dt.hour()); // https://stackoverflow.com/questions/76651472/do-rust-s3-sdk-datetimes-work-with-chrono
        let destination = PathBuf::from(&self.data_dir);
        let object = get_most_recent_object(&self.s3_client, &get_bucket(&self.sat_id), &prefix, last_object).await?;
        if let Some(obj) = object {
            self.latest_objs.insert(product.name.clone(), obj.clone());
            let data = get_goesr_data(&self.s3_client, obj, &destination, &product, self.sat_id.clone()).await?;
            let hotspots = read_goesr_data(&data)?;
            Ok(hotspots)
        } else {
            // try previous hour for case when we start the program before the data is up for the current hour (e.g., start at 5:00p - get error of no objects)
            let last_hour_object = get_last_hour_objects(&self.s3_client, &dt, &get_bucket(&self.sat_id), &product, last_object).await?;
            if let Some(obj) = last_hour_object {
                self.latest_objs.insert(product.name.clone(), obj.clone());
                let data = get_goesr_data(&self.s3_client, obj, &destination, &product, self.sat_id.clone()).await?;
                let hotspots = read_goesr_data(&data)?;
                Ok(hotspots)
            } else {
                Err(OdinGoesRError::NoObjectError(String::from("No objects for GOES-R product and datetime")))
            }
        }      
    }
    
    async fn sleep_for_remainder_of_cycle(&self) {
        sleep(minutes(5)).await;
    }
    pub async fn spawn_data_acquitision_task(&mut self) -> Result<()>{
        match  self.initial_download().await {
            Ok(init_hotspots) => {
                self.hself.send_msg( Initialize(init_hotspots) ).await?;
            }
            Err(e) => {
                error!("failed to download initial GOES-R data: {e:?}")
            }
        }
        loop {
            self.sleep_for_remainder_of_cycle().await;
            match  self.download_updates().await {
                Ok(hotspots) => {
                    self.hself.send_msg( Update(hotspots) ).await?;
                }
                Err(e) => {
                    error!("failed to download updated GOES-R data: {e:?}")
                }
            }
        }
    }
}