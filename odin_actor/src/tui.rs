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
#![allow(unused)]

use std::{error, io, panic};
use std::sync::Arc;
use futures::{FutureExt, StreamExt};
use kanal::AsyncSender;
use tokio::{sync::mpsc, select};
use crossterm::{event::{
    Event as CrosstermEvent, KeyCode, KeyEvent, MouseEvent, DisableMouseCapture, 
    EnableMouseCapture, KeyEventKind}, 
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen}
};
use ratatui::prelude::*;
use ratatui::{TerminalOptions,Viewport};
use ratatui::widgets::{Block, Tabs, Clear, Borders, BorderType};
use anyhow::{Error, Result};

use crate::{prelude::*, ActorSystemUITrait, errors, error};

mod actors_tab;
use actors_tab::ActorsTab;

mod messages_tab;
use messages_tab::MessagesTab;

mod theme;
use theme::THEME;

/// this represents the events that are forwarded to TUI from the actor system
pub enum TuiEvent {
    ActorsStarted,
    AddActor(Arc<String>, &'static str),
    RemoveActor(usize),
    NoStartActor(usize),
    HeartBeatsStarted,
    HeartBeatCycleStarted(u32),
    ActorHeartbeat(usize, u32, u64),
    UnresponsiveActor(usize),
    NoTermiateActor(usize),
    ActorsTerminated,
}

/// this is a wraper for the sending side of the channel, supposed to be provided 
/// to the actor system so it can forward sync events (TuiEvent) to TUI.
pub struct TuiHandle {
    tx: MpscSender<TuiEvent>,
}

impl TuiHandle {
    pub fn new_boxed(tx: MpscSender<TuiEvent>)->Box<Self> {
        Box::new(TuiHandle {tx})
    }
    
    pub fn send_event (&self, msg: TuiEvent) {
        if let Err(e) = self.tx.try_send(msg) {
            error!("try_send failed: {e:?}");
        }
    }
}

/// implementing ActorSystemUITrait which is an abstraction layer that actor system relies 
/// on to work with user interfaces. 
impl ActorSystemUITrait for TuiHandle {
    fn actors_started (&mut self) {
        self.send_event(TuiEvent::ActorsStarted);
    }

    fn add_actor (&mut self, id: Arc<String>, type_name: &'static str) {
        self.send_event(TuiEvent::AddActor(id, type_name));
    }

    fn remove_actor (&mut self, idx: usize) {
        self.send_event(TuiEvent::RemoveActor(idx));
    }

    fn no_start_actor (&mut self, idx: usize) {
        self.send_event(TuiEvent::NoStartActor(idx));
    }

    fn heartbeats_started (&mut self) {
        self.send_event(TuiEvent::HeartBeatsStarted);
    }

    fn heartbeat_cycle_started (&mut self, cycle: u32) {
        if cycle > 1 {
            self.send_event(TuiEvent::HeartBeatCycleStarted(cycle));
        }
    }

    fn actor_heartbeat (&mut self, idx: usize, cycle: u32, last_ns: u64) {
        self.send_event(TuiEvent::ActorHeartbeat(idx,cycle,last_ns));
    }

    fn unresponsive_actor (&mut self, idx: usize) {
        self.send_event(TuiEvent::UnresponsiveActor(idx));
    }

    fn no_terminate_actor (&mut self, idx: usize) {
        self.send_event(TuiEvent::NoTermiateActor(idx));
    }

    fn actors_terminated (&mut self) {
        self.send_event(TuiEvent::ActorsTerminated);
    }
}

/// This represents a terminal user interface. It is responsible for handling 
/// the events obtained from the actor system and the terminal. 
pub struct Tui {
    tx: MpscSender<TuiEvent>,
    rx: MpscReceiver<TuiEvent>,
    hsys: Arc<ActorSystemHandle>,
    tab: Tab,
    actors_tab: ActorsTab,
    messages_tab: MessagesTab,
}

impl Tui {
    pub fn new(hsys: Arc<ActorSystemHandle>) -> Self {
        let (atx, arx) = create_mpsc_sender_receiver::<TuiEvent>(20);
        Self {
            tx: atx,
            rx: arx,
            hsys,
            tab: Tab::Actors,
            actors_tab: ActorsTab::new(),
            messages_tab: MessagesTab::new(),
        }
    }

    /// it exits the terminal interface by disabling the raw mode and leaving the
    /// alternate screen
    pub fn exit(&mut self) -> io::Result<()> {
        restore_terminal()?;
        Ok(())
    }

    pub fn get_tui_handle (&mut self) -> Box<TuiHandle> {
        let tuih = TuiHandle::new_boxed(self.tx.clone());
        tuih
    }

    /// this is a key method that processes events received from the actor system
    pub async fn process_asys_events(&mut self, msg: TuiEvent, terminal: &mut Terminal<impl Backend>)->io::Result<()>{
        match msg {
            TuiEvent::ActorsStarted => {},
            TuiEvent::AddActor(id,type_name) => {
                self.actors_tab.add_actor(id, type_name);
            },
            TuiEvent::RemoveActor(idx) => {
                self.actors_tab.remove_actor(idx);
            },
            TuiEvent::NoStartActor(idx) => {},
            TuiEvent::HeartBeatsStarted => {},
            TuiEvent::HeartBeatCycleStarted(cycle) => {
                if cycle > 1 {
                    self.draw(terminal)?;
                }
            },
            TuiEvent::ActorHeartbeat(idx, cycle, last_ns) => {
                self.actors_tab.update_actor(idx, cycle, last_ns);
            },
            TuiEvent::UnresponsiveActor(idx) => {},
            TuiEvent::NoTermiateActor(idx) => {},
            TuiEvent::ActorsTerminated => {},
        }
        Ok(())
    }

    /// this is a key method that processes events received from the terminal
    pub async fn process_terminal_events(&mut self, key_event: KeyEvent, terminal: &mut Terminal<impl Backend>)->Result<()> {
        if key_event.kind == KeyEventKind::Press {
           match key_event.code {
               KeyCode::Char('q') | KeyCode::Esc => {
                  self.exit()?;
                  self.hsys.send_msg(crate::ActorSystemRequest::RequestTermination, secs(0)).await?;
               }
               KeyCode::Down => {
                  self.next();
                  self.draw(terminal)?;
               }
               KeyCode::Up => {
                  self.prev();
                  self.draw(terminal)?;
               }
               KeyCode::Left => {
                  self.prev_tab();
                  self.draw(terminal)?;
               }
               KeyCode::Right => {
                  self.next_tab();
                  self.draw(terminal)?;
               }
               _ => {}
           }
        }
        Ok(())
    }

    fn prev(&mut self) {
        match self.tab {
            Tab::Actors => self.actors_tab.prev(),
            Tab::Messages => {},
        }
    }

    fn next(&mut self) {
       match self.tab {
          Tab::Actors => self.actors_tab.next(),
          Tab::Messages => {},
       }
    }

    fn prev_tab(&mut self) {
       self.tab = self.tab.prev();
    }
    
    fn next_tab(&mut self) {
        self.tab = self.tab.next();
    }
   
    /// this is the main entry point for drawing to the terminal which draws a single frame 
    pub fn draw(&self, terminal: &mut Terminal<impl Backend>)->io::Result<()> {
        terminal
            .draw(|frame| frame.render_widget(self, frame.size()))?;
        Ok(())
    }


}

/// we implement Widget for &TUI instead of TUI as we would otherwise have to clone or copy the
/// entire TUI state on every frame.
impl Widget for &Tui {
    /// this is called by TUI.draw() which is a main entry point to render the entire 
    /// tui
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::bordered()
            .style(THEME.root)
            .border_set(BorderType::QuadrantOutside.to_border_set());
        block.render(area, buf);
        
        let area = area.inner( Margin {
            vertical: 1,
            horizontal: 1,
        });
        let vertical = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ]);
        let [title_bar, tab, command_bar] = vertical.areas(area);

        let block = Block::new().borders(Borders::TOP | Borders::BOTTOM);
        block.render(tab, buf);
        let tab = tab.inner( Margin {
            vertical: 1,
            horizontal: 1,
        });
        Clear.render(tab, buf);
        
        self.render_title_bar(title_bar, buf);
        self.render_selected_tab(tab, buf);
        Tui::render_command_bar(command_bar, buf);
    }
}

impl Tui {
    /// it renders the title bar which includes the title and the tabs 
    fn render_title_bar(&self, area: Rect, buf: &mut Buffer) {
        let layout = Layout::horizontal([Constraint::Min(10), Constraint::Length(20)]);
        let [title, tabs] = layout.areas(area);

        Span::styled(" ODIN-fire ", THEME.root_title)
            .render(title, buf);
        let tab_titles = Tab::TABS.map(Tab::title);

        Tabs::new(tab_titles)
            .style(THEME.tabs)
            .highlight_style(THEME.tabs_selected)
            .select(self.tab as usize)
            .divider(" ")
            .padding("", "")
            .render(tabs, buf);
    }

    /// it renders the content of the selected tab
    fn render_selected_tab(&self, area: Rect, buf: &mut Buffer) {
        match self.tab {
            Tab::Actors => self.actors_tab.render(area, buf),
            Tab::Messages => self.messages_tab.render(area, buf),
        };
    }

    /// it renders the command bar
    fn render_command_bar(area: Rect, buf: &mut Buffer) {
        let commands = [
            ("←", "Left"),
            ("→", "Right"),
            ("↑", "Up"),
            ("↓", "Down"),
            ("Q/Esc", "Quit"),
        ];

        let spans: Vec<Span> = commands
            .iter()
            .flat_map(|(key, desc)| {
                let command = Span::styled(format!(" {key} "), THEME.command_bar.command);
                let desc = Span::styled(format!(" {desc}  "), THEME.command_bar.description);
                [command, desc]
            }).collect();
        Line::from(spans)
            .centered()
            .style(THEME.command_bar.bar)
            .render(area, buf);
    }
}

/// the variants of the Tab enum represent the tabs that appear on the tui. 
/// Note that the tab instance must be also added to the associated const TAB::TABS
#[derive(Debug, Clone, Copy)]
enum Tab {
    Actors,
    Messages,
}

impl Tab {
    const TABS: [Self; 2] = [Self::Actors, Self::Messages];

    fn title(self)->String {
       format!(" {:?} ", self)
    }

    fn next(self)->Self {
        let current_index = self as usize;
        let next_index = current_index + 1;
        
        if next_index >= Tab::TABS.len() {
            Tab::TABS[current_index]
        } else {
            Tab::TABS[next_index]
        }
    }

    fn prev(self)->Self {
        let current_index = self as usize;
        let prev_index = current_index.saturating_sub(1);
        Tab::TABS[prev_index]
    }
}

/// this specifies the position and measurements of the tui on the terminal
const TUI_AREA: (u16, u16, u16, u16) = (0, 0, 105, 25);

/// it initializes the terminal interface by enabling the raw mode and entering 
/// an alternate screen.
pub fn init_terminal()->io::Result<Terminal<impl Backend>> {
    let options = TerminalOptions {
        viewport: Viewport::Fixed(Rect::new(TUI_AREA.0, TUI_AREA.1, TUI_AREA.2, TUI_AREA.3)),
    };
    
    let backend = CrosstermBackend::new(io::stderr());
    let mut terminal = Terminal::with_options(backend, options)?;
    
    terminal::enable_raw_mode()?;
    crossterm::execute!(io::stderr(), EnterAlternateScreen, EnableMouseCapture)?;

    let panic_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic| {
        restore_terminal().expect("failed to reset the terminal");
      panic_hook(panic);
   }));

    terminal.hide_cursor()?;
    terminal.clear()?;
    Ok(terminal)
}

/// it resets the terminal interface by disabling the raw mode and leaving the
/// alternate screen
pub fn restore_terminal()->io::Result<()> {
    terminal::disable_raw_mode()?;
    crossterm::execute!(io::stderr(), LeaveAlternateScreen, DisableMouseCapture)?;
    Ok(())
}

/// this is a key method that setups the tui and returns a handle (TuiHandle) to be used
/// by the actor system to work with the tui. This also spawns a task that initializes 
/// the terminal, starts the tui, and includes a loop which processes both terminal events 
/// and actor system events. It uses select! from tokio and it prioritizes the events comming 
/// from the terminal.
/// 
/// Note that this is consider the the tui entry point from the odin application. 
pub async fn create_tui (hsys: Arc<ActorSystemHandle>)->Result<Box<TuiHandle>> {
    let mut tui = Tui::new(hsys);
    let tuih = tui.get_tui_handle();

    tokio::spawn(async move {
       let terminal = &mut init_terminal().unwrap();
       tui.draw(terminal)?;

       let mut reader = crossterm::event::EventStream::new();
       loop {
          let terminal_event = reader.next().fuse();
          select! {
            biased;
            Some(Ok(term_evt)) = terminal_event => {
                match term_evt {
                    CrosstermEvent::Key(key) => {
                        if key.kind == crossterm::event::KeyEventKind::Press {
                            tui.process_terminal_events(key, terminal).await?;
                        }
                    },
                    CrosstermEvent::Mouse(mouse) => {},
                    CrosstermEvent::Resize(x, y) => {
                        terminal.clear()?;
                        tui.draw(terminal)?;
                    },
                    CrosstermEvent::FocusLost => {},
                    CrosstermEvent::FocusGained => {},
                    CrosstermEvent::Paste(_) => {},
                }
            }
            asys_event = recv(&tui.rx) => {
                if let Ok(asys_event) = asys_event {
                    tui.process_asys_events(asys_event, terminal).await?;
                } 
            }
        }
       }
       // this is unreachable, but had to add it for error propogation. See here: 
       // https://rust-lang.github.io/async-book/07_workarounds/02_err_in_async_blocks.html
       Ok::<(), Error>(())
   });
   Ok(tuih)
}