use std::{ffi::OsStr, path::PathBuf, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    process::{Child, Command},
    sync::mpsc::Receiver,
    task::JoinHandle,
};

/// Information of an engine.
pub struct Engine {
    /// Command to start the engine.
    cmd: Vec<String>,
    /// Self-reported name of the engine.
    pub info_name: String,
}

impl Engine {
    /// Create a new engine, and check that's it's actually a TEI engine.
    pub async fn new(cmd: Vec<String>) -> Self {
        assert!(!cmd.is_empty(), "Empty command line for engine");

        let process = Process::new(&cmd).await;
        let info_name = process.info_name.clone();
        process.quit().await;

        Engine { cmd, info_name }
    }

    pub async fn spawn(&self) -> Process {
        Process::new(&self.cmd).await
    }
}

/// Engine process.
pub struct Process {
    process: Child,
    pub stdout_recv: Receiver<String>,
    info_name: String,
    _task: JoinHandle<()>,
}
impl Process {
    pub async fn new<I: IntoIterator<Item = S>, S: AsRef<OsStr>>(cmd: I) -> Self {
        let mut cmd = cmd.into_iter();
        let path = cmd.next().expect("Empty command line for engine");
        let mut process = match Command::new(&path)
            .args(cmd)
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
            info_name: String::new(),
            stdout_recv,
            _task,
        };

        this.send("tei").await;

        while let Some(line) = this.stdout_recv.recv().await {
            if let Some(name) = line.strip_prefix("id name ") {
                this.info_name = name.to_string();
            }
            if line == "teiok" {
                break;
            }
        }

        this
    }

    pub async fn send(&mut self, command: &str) {
        let stdin = self.process.stdin.as_mut().unwrap();
        stdin.write_all(command.as_bytes()).await.unwrap();
        stdin.write_all(b"\n").await.unwrap();
    }

    pub async fn wait_ready(&mut self) {
        self.send("isready").await;
        while let Some(line) = self.stdout_recv.recv().await {
            if line == "readyok" {
                break;
            }
        }
    }

    pub async fn quit(mut self) {
        self.send("quit").await;
        let _ = self.process.wait().await;
    }
}
