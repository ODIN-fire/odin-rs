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
use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};

/// it defines the theme for the entire tui]
#[derive(Deserialize, Serialize, Debug)]
pub struct TuiTheme {
    pub main: MainTheme,
    pub actors_tab: ActorsTabTheme,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct MainTheme {
    pub root: Style,
    pub root_title: Style,
    pub tabs: Style,
    pub tabs_selected: Style,
    pub command_bar: CommandBar,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct CommandBar {
    pub command: Style,
    pub description: Style,
    pub bar: Style,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ActorsTabTheme {
    pub actor_table: ActorTable,
    pub chart: ActorBarChart,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ActorTable {
    pub header: Style,
    pub selected: Style,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ActorBarChart {
    pub ave_bar: Color,
    pub ave_val: Style,
    pub min_bar: Color,
    pub min_val: Style,
    pub max_bar: Color,
    pub max_val: Style,
    pub bar_width: u16,
    pub group_gap: u16,
}