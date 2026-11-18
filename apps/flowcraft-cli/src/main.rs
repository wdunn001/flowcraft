//! The FlowCraft command line: validate, inspect and run flow documents.
//!
//! This binary owns the IO the library crates refuse: files, and TCP connections to the
//! apps' loopback control ports. Cloud operations need a CollabCraft session the CLI does
//! not hold yet; `run` reports them clearly instead (docs/gaps.md).

mod diagram;

use clap::{Parser, Subcommand};
use flowcraft_engine::{Connector, ConnectorError, RunReport, StepStatus};
use flowcraft_flow::{Flow, validate};
use flowcraft_integrations::control::{ControlSessions, auth_line, parse_reply};
use flowcraft_integrations::{AppConnector, TransportError};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::error::Error;
use std::io::{BufRead, BufReader, Write as _};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "flowcraft-cli", version, about = "Workflows between the Craft apps and CollabCraft")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Write a starter flow document.
    New {
        file: PathBuf,
        #[arg(long, default_value = "Archive signed documents")]
        name: String,
    },
    /// Check a flow document's structure and references.
    Validate { file: PathBuf },
    /// Summarize a flow document.
    Inspect { file: PathBuf },
    /// Draw a flow document: a craft-flowchart diagram, auto-arranged.
    Diagram {
        file: PathBuf,
        /// Where to write the diagram; the default is standard output.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Run a flow. Without --targets this is a dry run.
    Run {
        file: PathBuf,
        /// JSON file with the trigger payload; the default is an empty object.
        #[arg(long)]
        trigger_payload: Option<PathBuf>,
        /// JSON file mapping app names to control ports and key files.
        #[arg(long)]
        targets: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    match dispatch(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            let mut cause = error.source();
            while let Some(inner) = cause {
                eprintln!("  caused by: {inner}");
                cause = inner.source();
            }
            ExitCode::FAILURE
        }
    }
}

fn dispatch(cli: Cli) -> Result<(), Box<dyn Error>> {
    match cli.command {
        Cmd::New { file, name } => new_flow(&file, &name),
        Cmd::Validate { file } => {
            validate(&load_flow(&file)?)?;
            println!("ok");
            Ok(())
        }
        Cmd::Inspect { file } => inspect(&load_flow(&file)?),
        Cmd::Diagram { file, out } => draw(&file, out.as_deref()),
        Cmd::Run { file, trigger_payload, targets } => run_flow(&file, trigger_payload.as_deref(), targets.as_deref()),
    }
}

fn load_flow(file: &Path) -> Result<Flow, Box<dyn Error>> {
    let text = std::fs::read_to_string(file)?;
    Ok(Flow::from_json_str(&text)?)
}

fn load_json(file: &Path) -> Result<Value, Box<dyn Error>> {
    let text = std::fs::read_to_string(file)?;
    Ok(serde_json::from_str(&text)?)
}

fn draw(file: &Path, out: Option<&Path>) -> Result<(), Box<dyn Error>> {
    let flow = load_flow(file)?;
    validate(&flow)?;
    let drawn = diagram::diagram_for(&flow);
    drawn.validate()?;
    let text = drawn.to_json_string()?;
    match out {
        None => println!("{text}"),
        Some(path) => {
            std::fs::write(path, text + "\n")?;
            println!("wrote {}", path.display());
        }
    }
    Ok(())
}

fn new_flow(file: &Path, name: &str) -> Result<(), Box<dyn Error>> {
    if file.exists() {
        return Err(format!("refusing to overwrite {}", file.display()).into());
    }
    let starter = json!({
        "format_version": flowcraft_flow::FORMAT_VERSION,
        "id": "starter",
        "name": name,
        "trigger": {"type": "app-event", "app": "pdfcraft", "event": "document.signed"},
        "steps": [
            {"id": "store", "name": "Archive the signed file", "action": {"type": "cloud", "operation": {
                "type": "store-file", "path": "signed/latest.pdf",
                "content": {"$ref": {"from": "trigger", "path": ["document"]}}
            }}},
            {"id": "notify", "name": "Tell the owner", "action": {"type": "cloud", "operation": {
                "type": "notify", "user": "owner",
                "message": {"$ref": {"from": "step", "step": "store", "path": []}}
            }}}
        ],
        "edges": [{"from": "store", "to": "notify"}]
    });
    let flow: Flow = serde_json::from_value(starter)?;
    validate(&flow)?;
    std::fs::write(file, flow.to_json_string()? + "\n")?;
    println!("wrote {}", file.display());
    Ok(())
}

fn inspect(flow: &Flow) -> Result<(), Box<dyn Error>> {
    validate(flow)?;
    println!("flow {}  ({})", flow.id, flow.name);
    println!("trigger: {}", serde_json::to_string(&flow.trigger)?);
    for step in &flow.steps {
        let kind = match &step.action {
            flowcraft_flow::Action::AppCommand { app, command, .. } => format!("app {app} {command}"),
            flowcraft_flow::Action::Cloud { .. } => "cloud".to_string(),
            flowcraft_flow::Action::Compose { .. } => "compose".to_string(),
        };
        println!("step {}  [{kind}]", step.id);
    }
    for edge in &flow.edges {
        let gate = if edge.when.is_some() { " (conditional)" } else { "" };
        println!("edge {} to {}{gate}", edge.from, edge.to);
    }
    Ok(())
}

fn run_flow(file: &Path, payload: Option<&Path>, targets: Option<&Path>) -> Result<(), Box<dyn Error>> {
    let flow = load_flow(file)?;
    let payload = match payload {
        Some(path) => load_json(path)?,
        None => json!({}),
    };
    let report = match targets {
        None => {
            eprintln!("no --targets given: dry run, nothing is called");
            flowcraft_engine::run(&flow, &payload, &mut DryRun)?
        }
        Some(path) => {
            let sessions = TcpSessions::load(path)?;
            flowcraft_engine::run(&flow, &payload, &mut AppConnector::new(sessions))?
        }
    };
    println!("{}", serde_json::to_string_pretty(&report_value(&report))?);
    Ok(())
}

fn report_value(report: &RunReport) -> Value {
    let steps: Vec<Value> = report
        .steps
        .iter()
        .map(|step| {
            let status = match step.status {
                StepStatus::Ran => "ran",
                StepStatus::Skipped => "skipped",
            };
            json!({"id": step.id, "status": status, "output": step.output})
        })
        .collect();
    json!({"flow": report.flow_id, "steps": steps})
}

/// The connector behind `run` without `--targets`: answers every step locally.
struct DryRun;

impl Connector for DryRun {
    fn app_command(&mut self, app: &str, command: &str, params: &Value) -> Result<Value, ConnectorError> {
        Ok(json!({"dry_run": true, "app": app, "command": command, "params": params}))
    }

    fn cloud(&mut self, operation: &Value) -> Result<Value, ConnectorError> {
        Ok(json!({"dry_run": true, "operation": operation}))
    }
}

/// One app's control target, from the `--targets` file:
///
/// ```json
/// {"apps": {"wordcraft": {"port": 7981, "key_file": "~/.config/wordcraft/control-key.local-7981"}}}
/// ```
///
/// `key` may be given inline instead of `key_file`; the key is never logged.
#[derive(Deserialize)]
struct Target {
    port: u16,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    key_file: Option<PathBuf>,
}

#[derive(Deserialize)]
struct Targets {
    apps: BTreeMap<String, Target>,
}

struct TcpSessions {
    targets: Targets,
}

impl TcpSessions {
    fn load(path: &Path) -> Result<Self, Box<dyn Error>> {
        let text = std::fs::read_to_string(path)?;
        Ok(Self { targets: serde_json::from_str(&text)? })
    }
}

fn target_key(target: &Target) -> Result<String, TransportError> {
    if let Some(key) = &target.key {
        return Ok(key.clone());
    }
    let Some(path) = &target.key_file else {
        return Err(TransportError::new("the target names neither key nor key_file"));
    };
    let key = std::fs::read_to_string(path).map_err(|error| TransportError::new(format!("cannot read the key file: {error}")))?;
    Ok(key.trim().to_string())
}

fn round_trip_line(stream: &mut TcpStream, reader: &mut BufReader<TcpStream>, line: &str) -> Result<String, TransportError> {
    let send = |error: std::io::Error| TransportError::new(format!("send failed: {error}"));
    stream.write_all(line.as_bytes()).map_err(send)?;
    stream.write_all(b"\n").map_err(send)?;
    let mut reply = String::new();
    let read = reader.read_line(&mut reply).map_err(|error| TransportError::new(format!("read failed: {error}")))?;
    if read == 0 {
        return Err(TransportError::new("the app closed the connection"));
    }
    Ok(reply)
}

impl ControlSessions for TcpSessions {
    fn round_trip(&mut self, app: &str, line: &str) -> Result<String, TransportError> {
        let Some(target) = self.targets.apps.get(app) else {
            return Err(TransportError::new(format!("no control target configured for {app:?}")));
        };
        let key = target_key(target)?;
        let mut stream = TcpStream::connect(("127.0.0.1", target.port)).map_err(|error| TransportError::new(format!("connect failed: {error}")))?;
        stream.set_read_timeout(Some(Duration::from_secs(30))).map_err(|error| TransportError::new(format!("socket setup failed: {error}")))?;
        let cloned = stream.try_clone().map_err(|error| TransportError::new(format!("socket setup failed: {error}")))?;
        let mut reader = BufReader::new(cloned);
        let auth = auth_line(0, &key).map_err(|error| TransportError::new(error.to_string()))?;
        let reply = round_trip_line(&mut stream, &mut reader, &auth)?;
        parse_reply(&reply, 0).map_err(|error| TransportError::new(format!("auth refused: {error}")))?;
        round_trip_line(&mut stream, &mut reader, line)
    }
}
