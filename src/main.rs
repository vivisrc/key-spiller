use std::{sync::mpsc, thread};

use serde::{Deserialize, Serialize};
use tokio::{runtime, sync::mpsc as async_mpsc, task::LocalSet};

mod grab;
mod ws;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum Event {
    Key {
        key: String,
        modifiers: Vec<String>,
        code: u32,
    },
    Text {
        value: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum Request {
    Grab { enabled: bool },
}

fn main() {
    let (message_tx, message_rx) = mpsc::channel();
    let (event_tx, event_rx) = async_mpsc::unbounded_channel();

    thread::spawn(move || grab::start_grabber(event_tx, message_rx));

    thread::spawn(|| {
        LocalSet::new().block_on(
            &runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
            ws::serve_socket(event_rx, message_tx),
        )
    });

    loop {
        thread::park();
    }
}
