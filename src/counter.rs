use std::time::Duration;

use anyhow::Result;
use ractor::{Actor, ActorRef, cast};
use tokio::{
    sync::oneshot::{self, Sender},
    task::{JoinHandle, spawn_blocking},
};

use crate::AppMessage;

pub struct Counter;

pub enum CounterMessage {
    IncrementCounter(u8),
}

#[derive(Debug)]
struct BlockTask {
    canceller: Sender<()>,
    handle: JoinHandle<Result<()>>,
}

#[derive(Default, Debug)]
pub struct CounterState {
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
            // Simulate CPU-bound work
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

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> std::result::Result<(), ractor::ActorProcessingErr> {
        if let Some(BlockTask { canceller, handle }) = state.prev.take() {
            if !handle.is_finished() {
                canceller.send(()).unwrap();
            }
            handle.await??;
        }

        Ok(())
    }
}
