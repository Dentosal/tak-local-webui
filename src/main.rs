#![deny(unused_must_use)]

mod engine;

use axum::{
    Form, Router,
    extract::State,
    http::StatusCode,
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
    routing::get,
};
use clap::Parser;
use futures_util::{StreamExt, stream::BoxStream};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap, convert::Infallible, fs, path::PathBuf, sync::Arc, time::Duration,
};
use tokio::{sync::Mutex, time::timeout};
use tower_http::services::ServeDir;

use crate::engine::Engine;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Check that all engines are working and exit
    #[arg(short, long)]
    check: bool,

    /// Enginelist file, one per file
    #[arg(short = 'F', long)]
    enginelist: Option<PathBuf>,

    /// Engines to load (added to enginelist if any)
    engines: Vec<String>,
}

struct AppState {
    engines: HashMap<String, Engine>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let mut engines = HashMap::new();

    let mut engine_commands = args.engines.clone();
    if let Some(path) = args.enginelist {
        engine_commands.extend(
            fs::read_to_string(path)
                .expect("Unable to read enginelist file")
                .lines()
                .map(|line| line.trim().to_owned())
                .filter(|line| !line.is_empty() && !line.starts_with('#')),
        );
    }

    for arg in engine_commands {
        let (name, cmd) = match arg.split_once('=') {
            Some((name, cmd)) => (Some(name), cmd),
            None => (None, arg.as_str()),
        };

        let cmd = shlex::split(cmd).expect("Invalid command line");
        let engine = timeout(Duration::new(1, 0), Engine::new(cmd))
            .await
            .expect("Engine initialization timed out");
        engines.insert(name.unwrap_or(&engine.info_name).to_owned(), engine);
    }

    if args.check {
        return;
    }

    if engines.is_empty() {
        eprintln!(
            "No engines configured, please provide at least one engine as NAME=PATH argument, or use -F FILE"
        );
        return;
    }

    let shared_state = Arc::new(Mutex::new(AppState { engines }));
    let static_files = ServeDir::new("./static");
    let app = Router::new()
        .route("/engines", get(get_engines))
        .with_state(shared_state.clone())
        .route("/bestmove", get(get_best_move))
        .with_state(shared_state.clone())
        .route("/ponder", get(ponder_best_move))
        .with_state(shared_state)
        .fallback_service(static_files);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8000")
        .await
        .unwrap();
    println!("Listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

async fn get_engines(State(state): State<Arc<Mutex<AppState>>>) -> Response {
    let state = state.lock().await;
    let mut engines: Vec<_> = state.engines.keys().cloned().collect();
    engines.sort();
    axum::Json(serde_json::json!(engines)).into_response()
}

#[derive(Deserialize)]
struct BestMoveParams {
    engine: String,
    tps: String,
}

#[derive(Serialize)]
struct MoveInfo {
    /// Score (centipawns?)
    score: i64,
    /// Principal variation
    pv: Vec<String>,
}

async fn get_best_move(
    State(state): State<Arc<Mutex<AppState>>>,
    Form(params): Form<BestMoveParams>,
) -> Response {
    let mut engine = {
        let state = state.lock().await;
        let Some(engine) = state.engines.get(&params.engine) else {
            return (
                StatusCode::NOT_FOUND,
                axum::Json(serde_json::json!({"error": "Engine not found"})),
            )
                .into_response();
        };
        engine.spawn().await
    };

    let size = params.tps.bytes().filter(|c| *c == b'/').count() + 1;

    engine.send(&format!("teinewgame {}", size)).await;
    engine.send(&format!("position tps {}", params.tps)).await;
    engine.wait_ready().await;
    engine.send("go movetime 1000").await;
    let mut info: Option<MoveInfo> = None;
    let re_info = regex::Regex::new(r"^info .*\bscore cp (\-?\d+).*\bpv (.+)").unwrap();
    while let Some(line) = engine.stdout_recv.recv().await {
        if let Some(caps) = re_info.captures(&line) {
            info = Some(MoveInfo {
                score: caps.get(1).unwrap().as_str().parse().unwrap(),
                pv: caps
                    .get(2)
                    .unwrap()
                    .as_str()
                    .split(" ")
                    .map(|s| s.to_owned())
                    .collect(),
            })
        }
        if let Some(bestmove) = line.strip_prefix("bestmove ") {
            return axum::Json(serde_json::json!({ "bestmove": bestmove, "info": info }))
                .into_response();
        }
    }
    panic!("Engine process terminated unexpectedly");
}

async fn ponder_best_move(
    State(state): State<Arc<Mutex<AppState>>>,
    Form(params): Form<BestMoveParams>,
) -> Sse<BoxStream<'static, Result<Event, Infallible>>> {
    let mut engine = {
        let state = state.lock().await;
        let Some(engine) = state.engines.get(&params.engine) else {
            let stream = async_stream::stream! {
                yield Ok(Event::default().event("error").data("engine not found"));
            };
            return Sse::new(stream.boxed());
        };
        engine.spawn().await
    };

    let size = params.tps.bytes().filter(|c| *c == b'/').count() + 1;

    engine.send(&format!("teinewgame {}", size)).await;
    engine.send(&format!("position tps {}", params.tps)).await;
    engine.wait_ready().await;
    engine.send("go infinite").await;

    let re_info = regex::Regex::new(r"^info .*\bscore cp (\-?\d+).*\bpv (.+)").unwrap();

    let stream = async_stream::stream! {
        let mut engine = engine; // Prevent drop until end of stream
        while let Some(line) = engine.stdout_recv.recv().await {
            if let Some(caps) = re_info.captures(&line) {
                let info = MoveInfo {
                    score: caps[1].parse().unwrap(),
                    pv: caps[2].split_whitespace().map(|s| s.to_owned()).collect(),
                };
                yield Ok(Event::default().event("info").data(
                    serde_json::to_string(&info).unwrap()
                ));
            }
        }
        println!("Ponder end");
    };

    Sse::new(stream.boxed()).keep_alive(KeepAlive::default())
}
