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

use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

pub struct MessagesTab; 

/// todo - a place holder for a tab which displays message flow in  the actor system
impl MessagesTab {
    pub fn new() -> Self {
        Self {}
    }
}

impl Widget for &MessagesTab {
    fn render(self, area: Rect, buf: &mut Buffer) {
    }
}