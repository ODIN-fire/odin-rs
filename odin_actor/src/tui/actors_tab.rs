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
use ratatui::{
    prelude::*, 
    widgets::{*, block::Title}
};
use std::{fmt, sync::Arc};

use crate::tui::theme::*;

/// it encapsulates the actor data to get displayed
struct ActorData {
    id: Arc<String>,
    type_name: &'static str,
    status: PingStatus
}

// Note that PingStatus is duplicated (See console_ui.PingStatus) since there is no pub access to update. 
// the two should merge.

/// it encapsulates the ping data for the heartbeats
pub struct PingStatus {
    pub last_cycle: u32,
    pub last_ns: u64,
    pub min_ns: u64,
    pub max_ns: u64,
    pub avg_ns: u64,
    pub outlier: usize,
    pub was_outlier: bool,
}

impl PingStatus {
    pub fn new ()->Self { PingStatus{ last_cycle: 0, last_ns: 0, min_ns: 0, max_ns: 0, avg_ns: 0, outlier: 0, was_outlier: false } }

    fn update (&mut self, cycle: u32, last_ns: u64) {
        if cycle > 1 {
            if last_ns > 10* self.avg_ns && !self.was_outlier { // ignore one outlier
                //println!("@@ outlier: {}", last_ns);
                self.outlier += 1;
                self.was_outlier = true;

            } else {
                if last_ns < self.min_ns { self.min_ns = last_ns }
                if last_ns > self.max_ns { self.max_ns = last_ns }
        
                // we could round here but 1 nano_sec is already more resolution than realistic
                if last_ns > self.avg_ns {
                    self.avg_ns = self.avg_ns + (last_ns - self.avg_ns)/cycle as u64;
                } else {
                    self.avg_ns = self.avg_ns - (self.avg_ns - last_ns)/cycle as u64;
                }

                self.was_outlier = false;
            }
        } else {
            self.min_ns = last_ns; self.max_ns = last_ns; self.avg_ns = last_ns;
        }
        
        self.last_cycle = cycle;
        self.last_ns = last_ns;
    }
}

/// it represents the actor tab that dispays a table and a chart including heartbeats data
pub struct ActorsTab {
    actors: Vec<ActorData>,
    row_index: usize,
}

impl ActorsTab {
    pub fn new()-> Self {
        Self {
            actors: Vec::new(), 
            row_index: 0,
        }
    }

    // the method add_actor, update_actor, and remove_actor are used to update the 
    // ActorDisplayData vector based on the changes that occur on the actor system side.
    // they are called by the tui to process the actor system events (process_asys_events()).
    
    pub fn add_actor(&mut self, id: Arc<String>, type_name: &'static str) {
        // we only display the actor state type and the actor message type between < and >
        let beg = type_name.find("<").unwrap_or(0);
        let end = type_name.find(">").unwrap_or(type_name.len()-1)+1;

        self.actors.push( ActorData { 
            id, 
            type_name: &type_name[beg..end], 
            status: PingStatus::new() 
        });
    }

    pub fn update_actor(&mut self, idx: usize, cycle: u32, last_ns: u64) {
        self.actors[idx].status.update(cycle, last_ns);
    }

    pub fn remove_actor(&mut self, idx: usize) {
        self.actors.remove(idx);
    }

    /// it changes the index representing the selected table row to the previous one
    pub fn prev(&mut self) {
        self.row_index = self.row_index.saturating_add(self.actors.len() - 1) % self.actors.len();
    }

    /// it changes the index representing the selected table row to the next one
    pub fn next(&mut self) {
        self.row_index = self.row_index.saturating_add(1) % self.actors.len();
    }
}

/// we implement Widget for &ActorsTab instead of ActorsTab to avoid clone or copy the entire state
/// on every frame
impl Widget for &ActorsTab {
    /// this is the main entry point to render the actors tab including the table, scrollbar and chart
    fn render(self, area: Rect, buf: &mut Buffer) {
        let vertical = Layout::vertical([
            Constraint::Min(5), 
            Constraint::Length(30)]
        );
        let [table_area, chart_area] = vertical.areas(area);

        render_actors_table(table_area, buf, self.row_index, &self.actors);
        render_scrollbar(table_area, buf, self.row_index, self.actors.len());

        render_bar_chart(chart_area, buf, &self.actors);
    }
}

fn render_actors_table(area: Rect, buf: &mut Buffer, selected: usize, actors: &Vec<ActorData>) {
    let header = [ "cycle", "id", "type", "response", "ave", "min", "max", "outlier"]
    .into_iter()
    .map(Cell::from)
    .collect::<Row>()
    .style(THEME.actor_table.header)
    .height(1);
 
    let mut rows = Vec::new();
    for actor in actors {
       let row = Row::new(vec![
          actor.status.last_cycle.to_string(), 
          (*actor.id).to_owned(), 
          actor.type_name.to_string(),
          actor.status.last_ns.to_string(), 
          actor.status.avg_ns.to_string(), 
          actor.status.min_ns.to_string(), 
          actor.status.max_ns.to_string(), 
          actor.status.outlier.to_string(),
       ]);
       rows.push(row.clone());
    }

    let t = Table::new(
       rows,
       [
          Constraint::Length(6),
          Constraint::Length(6),
          Constraint::Length(47),
          Constraint::Length(8),
          Constraint::Length(6),
          Constraint::Length(6),
          Constraint::Length(6),
          Constraint::Length(7),
       ],
    )
    .header(header)
    .highlight_style(THEME.actor_table.selected)
    .highlight_symbol(">> ")
    .highlight_spacing(HighlightSpacing::Always);

    let mut state = TableState::default().with_selected(selected);
    StatefulWidget::render(t, area, buf, &mut state);
 }

 fn render_scrollbar(area: Rect, buf: &mut Buffer, selected: usize, length: usize) {
    let sb = Scrollbar::default()
        .orientation(ScrollbarOrientation::VerticalLeft)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(None)
        .thumb_symbol("");
    let mut state = ScrollbarState::default()
       .content_length(length)
       .position(selected);
    let area = area.inner( Margin {
        vertical: 1,
        horizontal: 0,
    });
    StatefulWidget::render(sb, area, buf, &mut state);
}

fn render_bar_chart(area: Rect, buf: &mut Buffer, actors: &Vec<ActorData>) {
    let mut groups = vec!();
    for actor in actors {
        groups.push(BarGroup::default()
        .bars(&[
            Bar::default()
                .value(actor.status.avg_ns)
                .style(THEME.chart.ave_bar)
                .value_style(THEME.chart.ave_val)
                .label("ave".into()),
            Bar::default().value(actor.status.min_ns)
                .style(THEME.chart.min_bar)
                .value_style(THEME.chart.min_val)
                .label("min".into()),
            Bar::default().value(actor.status.max_ns)
                .style(THEME.chart.max_bar)
                .value_style(THEME.chart.max_val)
                .label("max".into()),
        ])
        .label(Line::from((*actor.id).to_owned()).centered()));
    }

    Block::new().borders(Borders::TOP)
            .title(Title::from(" Hearbeat Results ")
            .alignment(Alignment::Left))
            .render(area, buf);
    let mut chart = BarChart::default()
        .bar_width(THEME.chart.bar_width)
        .group_gap(THEME.chart.group_gap);

    for group in groups {
        chart = chart.data(group);
    }

    let area = area.inner( Margin {
        vertical: 2,
        horizontal: 2,
    });
    chart.render(area, buf);
}