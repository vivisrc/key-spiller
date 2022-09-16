use std::{
    env::{self, VarError},
    io,
    sync::mpsc,
};

use futures::{future, SinkExt, StreamExt};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{
        broadcast::{self, error::SendError},
        mpsc as async_mpsc,
    },
    task,
};
use tungstenite::Message;

use crate::{Event, Request};

pub async fn connection(
    stream: TcpStream,
    requests: mpsc::Sender<Request>,
    mut events: broadcast::Receiver<Event>,
) {
    let websocket = match tokio_tungstenite::accept_async(stream).await {
        Ok(websocket) => websocket,
        Err(error) => {
            eprintln!("Error accepting websocket connection: {}", error);
            return;
        }
    };

    let (mut sink, stream) = websocket.split();

    let incoming = stream.for_each(|message| async {
        if let Ok(Message::Text(text)) = message {
            if let Ok(request) = serde_json::from_str(&text) {
                requests.send(request).unwrap();
            }
        }
    });

    let outgoing = async move {
        while let Ok(event) = events.recv().await {
            match sink
                .send(Message::Text(serde_json::to_string(&event).unwrap()))
                .await
            {
                Ok(_) => (),
                Err(tungstenite::Error::Io(error)) => {
                    if error.kind() != io::ErrorKind::WouldBlock {
                        eprintln!("Error sending websocket message: {}", error);
                        return;
                    }
                }
                Err(tungstenite::Error::ConnectionClosed) => return,
                Err(error) => {
                    eprintln!("Error sending websocket message: {}", error);
                    return;
                }
            }
        }
    };

    future::join(incoming, outgoing).await;
}

pub async fn serve_socket(
    mut events: async_mpsc::UnboundedReceiver<Event>,
    requests: mpsc::Sender<Request>,
) {
    let addr = env::var("WS_BIND_ADDR").unwrap_or_else(|error| match error {
        VarError::NotPresent => "127.0.0.1:2100".to_string(),
        VarError::NotUnicode(_) => panic!("WS_BIND_ADDR is not UTF-8 (how)"),
    });
    let server = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|error| panic!("Failed to bind to {}: {}", addr, error));

    println!("Listening on {}", addr);

    let (tx, _) = broadcast::channel(128);

    task::spawn({
        let tx = tx.clone();
        async move {
            while let Some(mut event) = events.recv().await {
                while let Err(SendError(value)) = tx.send(event) {
                    event = value;
                }
            }
        }
    });

    loop {
        let stream = match server.accept().await {
            Ok((stream, _)) => stream,
            Err(_) => continue,
        };

        task::spawn_local(connection(stream, requests.clone(), tx.subscribe()));
    }
}
