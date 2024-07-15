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

/// it defines the theme for the entire tui
pub struct Theme {
    pub root: Style,
    pub root_title: Style,
    pub tabs: Style,
    pub tabs_selected: Style,
    pub command_bar: CommandBar,
    pub actor_table: ActorTable,
    pub chart: ActorBarChart,
}

pub struct CommandBar {
    pub command: Style,
    pub description: Style,
    pub bar: Style,
}

pub struct ActorTable {
    pub header: Style,
    pub selected: Style,
}

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

pub const THEME: Theme = Theme {
    root: Style::new().bg(Palette::BLACK),
    root_title: Style::new()
        .fg(Palette::WHITE)
        .add_modifier(Modifier::BOLD),
    tabs: Style::new().fg(Palette::MID_GRAY),
    tabs_selected: Style::new()
        .fg(Palette::WHITE)
        .bg(Palette::BLACK)
        .add_modifier(Modifier::BOLD)
        .add_modifier(Modifier::REVERSED),
    command_bar: CommandBar {
        command: Style::new().fg(Palette::BLACK).bg(Palette::DARK_GRAY),
        description: Style::new().fg(Palette::DARK_GRAY),
        bar: Style::new().bg(Palette::BLACK),
    },
    actor_table: ActorTable {
        header: Style::new()
            .fg(Palette::BLACK)
            .bg(Palette::LIGHT_GRAY)
            .add_modifier(Modifier::BOLD),
        selected: Style::new()
            .fg(Palette::YELLOW)
            .add_modifier(Modifier::BOLD)
    },
    chart: ActorBarChart {
        ave_bar: Palette::YELLOW,
        ave_val: Style::new()
            .fg(Palette::MID_GRAY)
            .bg(Palette::YELLOW),
        min_bar: Palette::ORANGE,
        min_val: Style::new()
            .fg(Palette::MID_GRAY)
            .bg(Palette::ORANGE),
        max_bar: Palette::RED,
        max_val: Style::new()
            .fg(Palette::BLACK)
            .bg(Palette::RED),
        bar_width: 5,
        group_gap: 3,
    }
};

pub struct Palette;
impl Palette {
    // white-gray-black theme
    pub const WHITE: Color = Color::Rgb(255,255,255);
    pub const LIGHT_GRAY: Color = Color::Rgb(220, 220, 220);
    pub const MID_GRAY: Color = Color::Rgb(128, 128, 128);
    pub const DARK_GRAY: Color = Color::Rgb(68, 68, 68);
    pub const BLACK: Color = Color::Rgb(0, 0, 0);
    // fire theme
    pub const YELLOW: Color = Color::Rgb(255, 207, 51);
    pub const ORANGE: Color = Color::Rgb(240, 127, 0);
    pub const RED: Color = Color::Rgb(212, 0, 0);
}