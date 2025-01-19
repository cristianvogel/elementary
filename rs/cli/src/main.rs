use elem::engine;

use std::sync::{Arc, Mutex};
use std::{env, io::Error};

use futures_util::{SinkExt, StreamExt, TryStreamExt};
use log::info;
use tinyaudio::prelude::*;
use tokio::net::{TcpListener, TcpStream};

fn main() {
    let _ = env_logger::try_init();
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8080".to_string());

    // Start the audio device
    let params = OutputDeviceParameters {
        channels_count: 2,
        sample_rate: 44100,
        channel_sample_count: 512,
    };

    let (engine_main, engine_proc) = engine::new_engine(44100.0, 512);
    let _device = run_output_device(params, {
        move |data| {
            for samples in data.chunks_mut(params.channels_count) {
                engine_proc.process(
                    samples.as_ptr(),
                    samples.as_mut_ptr(),
                    params.channels_count,
                    samples.len(),
                );
            }
        }
    });

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(run_event_loop_main(addr, engine_main))
        .expect("Failed to start event loop")
}

async fn run_event_loop_main(addr: String, engine_main: engine::MainHandle) -> Result<(), Error> {
    let shared_engine_main = Arc::new(Mutex::new(engine_main));

    let (first, second) = tokio::join!(
        tokio::spawn(run_event_poller(shared_engine_main.clone())),
        tokio::spawn(run_tcp_listener(addr, shared_engine_main.clone())),
    );

    first.unwrap_or(second.unwrap_or(Ok(())))
}

async fn run_event_poller(engine_main: Arc<Mutex<engine::MainHandle>>) -> Result<(), Error> {
    let mut interval =
        tokio::time::interval(tokio::time::Duration::from_millis((1000.0 / 30.0) as u64));

    loop {
        interval.tick().await;

        if let Ok(result) = engine_main.lock().unwrap().process_queued_events() {
            if let Some(events) = result.as_array() {
                for evt in events.iter() {
                    println!("[Event] {}", evt.to_string());
                }
            }
        }
    }
}

async fn run_tcp_listener(
    addr: String,
    engine_main: Arc<Mutex<engine::MainHandle>>,
) -> Result<(), Error> {
    // Create the TCP listener we'll accept connections on
    let try_socket = TcpListener::bind(&addr).await;
    let listener = try_socket.expect("Failed to bind");
    info!("Listening on: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(accept_connection(stream, engine_main.clone()));
    }

    Ok(())
}

async fn accept_connection(stream: TcpStream, engine_main: Arc<Mutex<engine::MainHandle>>) {
    let addr = stream
        .peer_addr()
        .expect("connected streams should have a peer address");
    info!("Peer address: {}", addr);

    let ws_stream = tokio_tungstenite::accept_async(stream)
        .await
        .expect("Error during the websocket handshake occurred");

    info!("New WebSocket connection: {}", addr);

    let (mut write, mut read) = ws_stream.split();

    while let Ok(next) = read.try_next().await {
        if let Some(msg) = next {
            match msg.to_text() {
                Ok(text) => {
                    println!("Received a message from {}: {}", addr, text);
                    let directive: server::UnresolvedDirective =
                        serde_json::from_str(text).unwrap_or_default();
                    let resolved = server::resolve_directive(directive).await;

                    {
                        let mut main = engine_main.lock().unwrap();
                        let _ = main.render(resolved);
                    }

                    // TODO: Properly handle the write failure case
                    write.send(msg).await.unwrap()
                }
                Err(e) => {
                    println!("Received a non-text message from {}: {}", addr, e);
                    write.send("No thanks".into()).await.unwrap()
                }
            }
        }
    }

    println!("Connection closed to peer {}", addr);
}
