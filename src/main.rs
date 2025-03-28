#![allow(dead_code)]
use std::{
    io::{self, Stdout},
    sync::Arc,
    time::Duration,
};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ractor::{Actor, ActorRef, RpcReplyPort, call, cast};
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
use tokio::{
    sync::{
        Mutex,
        oneshot::{self, Sender},
    },
    task::{JoinHandle, spawn_blocking},
};

#[tokio::main]
async fn main() -> Result<()> {
    let (nb, _guard) =
        tracing_appender::non_blocking(tracing_appender::rolling::daily("./", "tui"));
    tracing_subscriber::fmt().with_writer(nb).init();
    let terminal = ratatui::init();
    let (app, app_handle) =
        Actor::spawn(Some("app".to_string()), App, AppArgs { tui: terminal }).await?;

    let (counter, counter_handle) = Actor::spawn(Some("counter".to_string()), Counter, ()).await?;

    while !call!(app, AppMessage::Exit)? {
        cast!(app, AppMessage::Draw)?;
        match event::read()? {
            // it's important to check that the event is a key press event as
            // crossterm also emits key release and repeat events on Windows.
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                tracing::info!("Got a key event {:?}", key_event);
                cast!(app, AppMessage::HandleKey(key_event))?;
                tracing::info!("Fired a key event {:?}", key_event);
            }
            _ => {}
        };
    }
    tracing::info!("Stopping app actor");
    app.stop(None);
    counter.stop(None);
    tracing::info!("Exited, awaiting handle");
    app_handle.await?;
    counter_handle.await?;
    tracing::info!("Handle ended");
    ratatui::restore();
    tracing::info!("Terminal restored");
    Ok(())
}

struct Counter;

enum CounterMessage {
    IncrementCounter(u8),
}

#[derive(Default, Debug)]
struct CounterState {
    prev: Option<BlockTask>,
}

impl Actor for Counter {
    type Msg = CounterMessage;

    type State = CounterState;

    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ractor::ActorRef<Self::Msg>,
        _args: Self::Arguments,
    ) -> Result<Self::State, ractor::ActorProcessingErr> {
        Ok(CounterState::default())
    }

    async fn handle(
        &self,
        _myself: ractor::ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> std::result::Result<(), ractor::ActorProcessingErr> {
        if let Some(BlockTask { canceller, handle }) = state.prev.take() {
            tracing::info!("Handling previous task");
            if !handle.is_finished() {
                tracing::info!("Not yet finished; cancelling");
                canceller.send(()).unwrap();
            }
            tracing::info!("Awaiting task");
            handle.await??;
        }
        tracing::info!("Incrementing counter");
        let CounterMessage::IncrementCounter(cur) = message;

        let app: ActorRef<AppMessage> = ractor::registry::where_is("app".to_string())
            .expect("App??")
            .into();

        let (send, mut recv) = oneshot::channel::<()>();

        let prev: JoinHandle<Result<()>> = spawn_blocking(move || {
            for _ in 0..10 {
                std::thread::sleep(Duration::from_secs(1));
                if let Ok(()) = recv.try_recv() {
                    tracing::info!("Got cancellation token");
                    return Ok(());
                }
            }
            tracing::info!("Finished waiting");
            cast!(app, AppMessage::UpdateCount(cur + 1))?;

            Ok(())
        });

        state.prev = Some(BlockTask {
            canceller: send,
            handle: prev,
        });

        Ok(())
    }
}

#[derive(Debug)]
struct BlockTask {
    canceller: Sender<()>,
    handle: JoinHandle<Result<()>>,
}

struct App;

struct AppArgs {
    tui: Terminal<CrosstermBackend<Stdout>>,
}

enum AppMessage {
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

    /// updates the application's state based on user input
    fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            // it's important to check that the event is a key press event as
            // crossterm also emits key release and repeat events on Windows.
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.handle_key_event(key_event);
            }
            _ => {}
        };
        Ok(())
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

    fn increment_counter(&mut self) {
        self.counter += 1;
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

#[cfg(test)]
mod tests {}
