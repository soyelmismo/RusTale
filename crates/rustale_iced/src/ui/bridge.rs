use crate::messages::Message;
use crate::core::signals::FromCore;
use iced::Subscription;
use iced::advanced::subscription::{self, Recipe};
use iced::futures::StreamExt;
use std::hash::Hash;

pub fn subscribe(
    core_receiver: std::sync::Arc<std::sync::Mutex<Option<tokio::sync::mpsc::Receiver<FromCore>>>>,
) -> Subscription<Message> {
    subscription::from_recipe(CoreSubscription { core_receiver })
}

struct CoreSubscription {
    core_receiver: std::sync::Arc<std::sync::Mutex<Option<tokio::sync::mpsc::Receiver<FromCore>>>>,
}

impl Recipe for CoreSubscription {
    type Output = Message;

    fn hash(&self, state: &mut iced::advanced::subscription::Hasher) {
        use std::any::TypeId;
        TypeId::of::<Self>().hash(state);
    }

    fn stream(
        self: Box<Self>,
        _input: subscription::EventStream,
    ) -> iced::futures::stream::BoxStream<'static, Self::Output> {
        let core_receiver = self.core_receiver;

        use iced::futures::SinkExt;

        iced::stream::channel(
            100,
            |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
                let rx = {
                    let mut guard = core_receiver.lock().unwrap();
                    guard.take()
                };

                if let Some(mut rx) = rx {
                    while let Some(msg) = rx.recv().await {
                        let _ = output.send(Message::CoreEvent(msg)).await;
                    }
                } else {
                    std::future::pending::<()>().await;
                }
            },
        )
        .boxed()
    }
}
