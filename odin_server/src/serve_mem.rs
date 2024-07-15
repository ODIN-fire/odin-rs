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
#![allow(unused)]

use std::{collections::HashMap,convert::Infallible, future::{Future,Ready,ready}, task::{Context,Poll}, sync::Arc, result::Result};
use http::{header, HeaderValue, Method, Request, Response, StatusCode,Uri};
use tower::Service;
use http_body_util::{Full,Empty,Either};
use bytes::Bytes;

type ResponseBody = Full<Bytes>; // we already have the data in memory, no use to stream it (?? we need to copy_from_slice)

/// note this needs to clone efficiently
#[derive(Clone)]
pub struct ServeMem {
    pub dict: Arc<HashMap<&'static str,&'static[u8]>>
}

impl ServeMem {
    pub fn new (dict: Arc<HashMap<&'static str,&'static[u8]>>)->Self {
        ServeMem { dict }
    }
}

impl<ReqBody> Service<Request<ReqBody>> for ServeMem
    where ReqBody: Send + 'static,
{
    type Response = Response<ResponseBody>;
    type Error = Infallible;
    type Future = Ready<Result<Response<ResponseBody>,Infallible>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(())) // it's always ready
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let mut uri_str = req.uri().path();
        let key = if uri_str.starts_with("/") { // if this is used from nest_service the uri is modified but still starts with '/'
            uri_str.strip_prefix("/").unwrap()
        } else {
            uri_str // if this is called from a route(Path(...),..) handler the path does NOT start with '/'
        };

        match self.dict.get( key) {
            Some(data) => {
                let response = Response::builder()
                    .status(StatusCode::OK)
                    .body(Full::new(Bytes::copy_from_slice(data)))
                    .unwrap();
                ready(Ok(response))
            }
            None => {
                let response = Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Full::new(Bytes::new()))
                    .unwrap();
                ready(Ok(response))
            }
        }
    }
}