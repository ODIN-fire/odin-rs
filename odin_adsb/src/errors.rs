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

 use thiserror::Error;

 pub type Result<T> = std::result::Result<T,OdinAdsbError>;


#[derive(Error,Debug)]
pub enum OdinAdsbError {

    #[error("parse error {0}")]
    ParseError(String),

    #[error("IO error {0}")]
    IOError( #[from] std::io::Error),

    #[error("ODIN actor error {0}")]
    OdinActorError( #[from] odin_actor::OdinActorError),

    #[error("chrono parse error {0}")]
    ChronoParseError( #[from] chrono::ParseError),

    #[error("action error {0}")]
    ActionError( String ),

    #[error("operation failed {0}")]
    OpFailedError(String)
}

odin_action::map_action_failure!{ OdinAdsbError, ActionError}

macro_rules! parse_error {
    ($fmt:literal $(, $arg:expr )* ) => {
        OdinAdsbError::ParseError( format!( $fmt $(, $arg)* ))
    };
}
pub (crate) use parse_error;