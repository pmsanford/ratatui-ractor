mod app;
mod counter;

use anyhow::Result;
use counter::Counter;
use crossterm::event::{self, Event, KeyEventKind};
use ractor::{Actor, call, cast};

use app::{App, AppArgs, AppMessage};

#[tokio::main]
async fn main() -> Result<()> {
    let (nb, _guard) =
        tracing_appender::non_blocking(tracing_appender::rolling::daily("./", "tui"));
    tracing_subscriber::fmt().with_writer(nb).init();
    let terminal = ratatui::init();
    let (app, app_handle) =
        Actor::spawn(Some("app".to_string()), App, AppArgs { tui: terminal }).await?;

    let (counter, counter_handle) = Actor::spawn(Some("counter".to_string()), Counter, ()).await?;

    while !call!(app, AppMessage::ShouldExit)? {
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
