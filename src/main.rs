#![deny(unused_must_use)]

use axum::{
    extract::State,
    response::Response,
    {Form, Router},
    {http::StatusCode, response::IntoResponse, routing::get},
};
use clap::Parser;
use serde::Deserialize;
use std::{
    collections::HashMap, ffi::OsStr, fs, path::PathBuf, process::Stdio, sync::Arc, time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    process::{Child, Command},
    sync::{Mutex, mpsc::Receiver},
    task::JoinHandle,
    time::timeout,
};
use tower_http::services::ServeDir;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Check that all engines are working and exit
    #[arg(short, long)]
    check: bool,

    /// Enginelist file, one per file
    #[arg(short='F', long)]
    enginelist: Option<PathBuf>,

    /// Engines to load (added to enginelist if any)
    engines: Vec<String>,
}

pub struct Engine {
    process: Child,
    name: String,
    stdout_recv: Receiver<String>,
    _task: JoinHandle<()>,
}
impl Engine {
    pub async fn new<P: AsRef<OsStr>, I: IntoIterator<Item = S>, S: AsRef<OsStr>>(
        path: P,
        args: I,
    ) -> Self {
        let mut process = match Command::new(&path)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(process) => process,
            Err(e) => panic!(
                "Unable to start engine {}: {e}",
                PathBuf::from(path.as_ref()).display()
            ),
        };

        let stdout = process.stdout.take().unwrap();
        let (stdout_send, stdout_recv) = tokio::sync::mpsc::channel(100);
        let _task = tokio::spawn(async move {
            let reader = tokio::io::BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if stdout_send.send(line).await.is_err() {
                    break;
                }
            }
        });

        let mut this = Self {
            process,
            name: String::new(),
            stdout_recv,
            _task,
        };

        this.send("tei").await;

        while let Some(line) = this.stdout_recv.recv().await {
            if let Some(name) = line.strip_prefix("id name ") {
                this.name = name.to_string();
            }
            if line == "teiok" {
                break;
            }
        }

        this
    }

    async fn send(&mut self, command: &str) {
        let stdin = self.process.stdin.as_mut().unwrap();
        stdin.write_all(command.as_bytes()).await.unwrap();
        stdin.write_all(b"\n").await.unwrap();
    }

    async fn wait_ready(&mut self) {
        self.send("isready").await;
        while let Some(line) = self.stdout_recv.recv().await {
            if line == "readyok" {
                break;
            }
        }
    }
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
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
        );
    }

    for arg in engine_commands {
        let (name, cmd) = match arg.split_once('=') {
            Some((name, cmd)) => (Some(name), cmd),
            None => (None, arg.as_str()),
        };

        let mut args = shlex::split(cmd).expect("Invalid command line");
        let cmd = args.remove(0); // remove program name
        let engine = timeout(Duration::new(1, 0), Engine::new(cmd, &args))
            .await
            .expect("Engine initialization timed out");
        engines.insert(name.unwrap_or(&engine.name).to_owned(), engine);
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

async fn get_best_move(
    State(state): State<Arc<Mutex<AppState>>>,
    Form(params): Form<BestMoveParams>,
) -> Response {
    let mut state = state.lock().await;
    let Some(engine) = state.engines.get_mut(&params.engine) else {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({"error": "Engine not found"})),
        )
            .into_response();
    };

    let size = params.tps.bytes().filter(|c| *c == b'/').count() + 1;

    engine.send(&format!("teinewgame {}", size)).await;
    engine.send(&format!("position tps {}", params.tps)).await;
    engine.wait_ready().await;
    engine.send("go movetime 1000").await;
    while let Some(line) = engine.stdout_recv.recv().await {
        if let Some(bestmove) = line.strip_prefix("bestmove ") {
            return axum::Json(serde_json::json!({ "bestmove": bestmove })).into_response();
        }
    }
    panic!("Engine process terminated unexpectedly");
}
