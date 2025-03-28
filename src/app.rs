use std::{io::Stdout, sync::Arc};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ractor::{Actor, ActorRef, RpcReplyPort, cast};
use ratatui::{
    Frame, Terminal,
    buffer::Buffer,
    layout::Rect,
    prelude::CrosstermBackend,
    style::Stylize,
    symbols::border,
    text::{Line, Text},
    widgets::{Block, Paragraph, Widget},
};
use tokio::sync::Mutex;

use crate::counter::CounterMessage;

pub struct App;

pub struct AppArgs {
    pub tui: Terminal<CrosstermBackend<Stdout>>,
}

pub enum AppMessage {
    Draw,
    UpdateCount(u8),
    HandleKey(KeyEvent),
    Exit(RpcReplyPort<bool>),
}

impl Actor for App {
    type Msg = AppMessage;

    type State = AppState;

    type Arguments = AppArgs;

    async fn pre_start(
        &self,
        _myself: ractor::ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ractor::ActorProcessingErr> {
        Ok(AppState {
            counter: 0,
            exit: false,
            tui: Arc::new(Mutex::new(args.tui)),
        })
    }

    async fn handle(
        &self,
        myself: ractor::ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ractor::ActorProcessingErr> {
        match message {
            AppMessage::Draw => {
                tracing::info!("Drawing screen");
                let mut tui = state.tui.lock().await;
                tui.draw(|frame| state.draw(frame))?;
                tracing::info!("Drew screen");
            }
            AppMessage::UpdateCount(new) => {
                tracing::info!("Got counter update: {}", new);
                state.counter = new;
                tracing::info!("Sending draw request");
                cast!(myself, AppMessage::Draw)?;
                tracing::info!("Assigned counter update: {}", new);
            }
            AppMessage::Exit(reply) => {
                tracing::info!("Got exit check");
                reply.send(state.exit)?;
                tracing::info!("Replied to exit check");
            }
            AppMessage::HandleKey(evt) => {
                tracing::info!("Got key event {:?}", evt);
                state.handle_key_event(evt);
                tracing::info!("Handled key event {:?}", evt);
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct AppState {
    counter: u8,
    exit: bool,
    tui: Arc<Mutex<Terminal<CrosstermBackend<Stdout>>>>,
}

impl AppState {
    fn draw(&self, frame: &mut Frame) {
        frame.render_widget(self, frame.area());
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') => self.exit(),
            KeyCode::Left => self.decrement_counter(),
            KeyCode::Right => {
                let ctr: ActorRef<CounterMessage> =
                    ractor::registry::where_is("counter".to_string())
                        .expect("Counter???")
                        .into();
                cast!(ctr, CounterMessage::IncrementCounter(self.counter)).unwrap();
            }
            _ => {}
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }

    fn decrement_counter(&mut self) {
        self.counter -= 1;
    }
}

impl Widget for &AppState {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let title = Line::from(" Counter App Tutorial ".bold());
        let instructions = Line::from(vec![
            " Decrement ".into(),
            "<Left>".blue().bold(),
            " Increment ".into(),
            "<Right>".blue().bold(),
            " Quit ".into(),
            "<Q> ".blue().bold(),
        ]);
        let block = Block::bordered()
            .title(title.centered())
            .title_bottom(instructions.centered())
            .border_set(border::THICK);

        let counter_text = Text::from(vec![Line::from(vec![
            "Value: ".into(),
            self.counter.to_string().yellow(),
        ])]);

        Paragraph::new(counter_text)
            .centered()
            .block(block)
            .render(area, buf);
    }
}
