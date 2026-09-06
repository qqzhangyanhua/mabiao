use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(test)]
use std::collections::VecDeque;
#[cfg(test)]
use std::sync::Mutex;

use crate::domain::{DetectedEngine, EngineCommand, EngineProfile, WORK_NOTES_ENGINE_DIR};

const TIMEOUT: Duration = Duration::from_secs(180);
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);
const WORK_DIR_NAME: &str = WORK_NOTES_ENGINE_DIR;
const GROK_DISALLOWED_TOOLS: &str = "read_file,search_replace,grep,list_dir,run_terminal_command,run_terminal_cmd,web_search,web_fetch,todo_write,spawn_subagent,memory_search,Agent";

struct EngineSpec {
    id: &'static str,
    program: &'static str,
    writes_session_dir: bool,
    concurrency: u32,
    build: fn(&Path, SchemaKind, String, Option<&str>) -> Result<EngineCommand, String>,
}

impl EngineSpec {
    fn profile(&self) -> EngineProfile {
        EngineProfile {
            id: self.id.to_string(),
            program: self.program.to_string(),
            writes_session_dir: self.writes_session_dir,
            concurrency: self.concurrency,
        }
    }

    fn detected(&self, installed: bool, version: Option<String>) -> DetectedEngine {
        let profile = self.profile();
        DetectedEngine {
            id: profile.id,
            program: profile.program,
            writes_session_dir: profile.writes_session_dir,
            installed,
            version,
        }
    }
}

const ENGINES: &[EngineSpec] = &[
    EngineSpec {
        id: "codex",
        program: "codex",
        writes_session_dir: false,
        concurrency: 3,
        build: codex_command,
    },
    EngineSpec {
        id: "claude",
        program: "claude",
        writes_session_dir: false,
        concurrency: 3,
        build: claude_command,
    },
    EngineSpec {
        id: "grok",
        program: "grok",
        writes_session_dir: true,
        concurrency: 3,
        build: grok_command,
    },
    EngineSpec {
        id: "cursor-agent",
        program: "cursor-agent",
        writes_session_dir: true,
        concurrency: 3,
        build: cursor_agent_command,
    },
];

pub fn writes_session_dir(engine_id: &str) -> bool {
    spec(engine_id)
        .map(|item| item.writes_session_dir)
        .unwrap_or(false)
}

fn spec(engine_id: &str) -> Result<&'static EngineSpec, String> {
    ENGINES
        .iter()
        .find(|item| item.id == engine_id)
        .ok_or_else(|| format!("未知的纪要引擎：{engine_id}"))
}

pub fn require(engine_id: &str) -> Result<(), String> {
    spec(engine_id).map(|_| ())
}

pub fn command(
    engine_id: &str,
    work_dir: &Path,
    schema: SchemaKind,
    stdin: String,
    model: Option<&str>,
) -> Result<EngineCommand, String> {
    let spec = spec(engine_id)?;
    (spec.build)(work_dir, schema, stdin, model)
}

pub fn detect_engines() -> Vec<DetectedEngine> {
    detect_with(which_named, read_version)
}

pub fn detect_with(
    which: impl Fn(&str) -> Option<PathBuf>,
    version: impl Fn(&Path) -> Result<String, String>,
) -> Vec<DetectedEngine> {
    ENGINES
        .iter()
        .map(|item| match which(item.program) {
            Some(path) => item.detected(true, version(&path).ok().map(|raw| first_line(&raw))),
            None => item.detected(false, None),
        })
        .collect()
}

pub trait EngineRunner {
    fn run(&self, command: &EngineCommand) -> Result<String, String>;
}

pub struct ProcessRunner;

impl EngineRunner for ProcessRunner {
    fn run(&self, command: &EngineCommand) -> Result<String, String> {
        spawn(command)
    }
}

#[cfg(test)]
pub struct ScriptedRunner {
    replies: Mutex<VecDeque<Result<String, String>>>,
    recorded: Mutex<Vec<EngineCommand>>,
}

#[cfg(test)]
impl ScriptedRunner {
    pub fn succeeding(outputs: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            replies: Mutex::new(outputs.into_iter().map(|item| Ok(item.into())).collect()),
            recorded: Mutex::new(Vec::new()),
        }
    }

    pub fn recorded(&self) -> Vec<EngineCommand> {
        self.recorded.lock().expect("scripted runner").clone()
    }
}

#[cfg(test)]
impl EngineRunner for ScriptedRunner {
    fn run(&self, command: &EngineCommand) -> Result<String, String> {
        self.recorded
            .lock()
            .expect("scripted runner")
            .push(command.clone());
        self.replies
            .lock()
            .expect("scripted runner")
            .pop_front()
            .unwrap_or_else(|| Err("没有预置的引擎输出".to_string()))
    }
}

pub enum SchemaKind {
    Map,
    Reduce,
}

pub fn ensure_work_dir(app_data_dir: &Path) -> Result<PathBuf, String> {
    let dir = app_data_dir.join(WORK_DIR_NAME);
    fs::create_dir_all(&dir).map_err(|error| format!("无法创建纪要工作目录：{error}"))?;
    Ok(dir)
}

pub fn write_schemas(work_dir: &Path) -> Result<(), String> {
    fs::write(work_dir.join("map.schema.json"), MAP_SCHEMA)
        .map_err(|error| format!("无法写入 map schema：{error}"))?;
    fs::write(work_dir.join("reduce.schema.json"), REDUCE_SCHEMA)
        .map_err(|error| format!("无法写入 reduce schema：{error}"))?;
    Ok(())
}

pub fn claude_command(
    work_dir: &Path,
    schema: SchemaKind,
    stdin: String,
    model: Option<&str>,
) -> Result<EngineCommand, String> {
    let mut args = vec![
        "-p".to_string(),
        "--no-session-persistence".to_string(),
        "--tools".to_string(),
        String::new(),
        "--json-schema".to_string(),
        schema_json(schema).to_string(),
    ];
    push_model(&mut args, model);
    Ok(EngineCommand {
        program: "claude".to_string(),
        args,
        stdin,
        cwd: work_dir.to_path_buf(),
        session_id: None,
    })
}

pub fn grok_command(
    work_dir: &Path,
    schema: SchemaKind,
    stdin: String,
    model: Option<&str>,
) -> Result<EngineCommand, String> {
    let session_id = new_session_uuid();
    let mut args = vec![
        "-p".to_string(),
        stdin,
        "--json-schema".to_string(),
        schema_json(schema).to_string(),
        "--disallowed-tools".to_string(),
        GROK_DISALLOWED_TOOLS.to_string(),
        "-s".to_string(),
        session_id.clone(),
    ];
    push_model(&mut args, model);
    Ok(EngineCommand {
        program: "grok".to_string(),
        args,
        stdin: String::new(),
        cwd: work_dir.to_path_buf(),
        session_id: Some(session_id),
    })
}

pub fn cursor_agent_command(
    work_dir: &Path,
    _schema: SchemaKind,
    stdin: String,
    model: Option<&str>,
) -> Result<EngineCommand, String> {
    let mut args = vec![
        "-p".to_string(),
        "--mode".to_string(),
        "ask".to_string(),
        "--trust".to_string(),
    ];
    push_model(&mut args, model);
    args.push(stdin);
    Ok(EngineCommand {
        program: "cursor-agent".to_string(),
        args,
        stdin: String::new(),
        cwd: work_dir.to_path_buf(),
        session_id: None,
    })
}

pub fn codex_command(
    work_dir: &Path,
    schema: SchemaKind,
    stdin: String,
    model: Option<&str>,
) -> Result<EngineCommand, String> {
    let schema_name = match schema {
        SchemaKind::Map => "map.schema.json",
        SchemaKind::Reduce => "reduce.schema.json",
    };
    let schema_path = work_dir.join(schema_name);
    let schema_arg = schema_path
        .to_str()
        .ok_or_else(|| "纪要工作目录路径不是合法 UTF-8".to_string())?
        .to_string();
    let mut args = vec![
        "-a".to_string(),
        "never".to_string(),
        "exec".to_string(),
        "--ephemeral".to_string(),
        "-s".to_string(),
        "read-only".to_string(),
        "--skip-git-repo-check".to_string(),
        "--color".to_string(),
        "never".to_string(),
        "--output-schema".to_string(),
        schema_arg,
    ];
    push_model(&mut args, model);
    args.push("-".to_string());
    Ok(EngineCommand {
        program: "codex".to_string(),
        args,
        stdin,
        cwd: work_dir.to_path_buf(),
        session_id: None,
    })
}

fn schema_json(schema: SchemaKind) -> &'static str {
    match schema {
        SchemaKind::Map => MAP_SCHEMA,
        SchemaKind::Reduce => REDUCE_SCHEMA,
    }
}

fn new_session_uuid() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&nanos.to_be_bytes());
    bytes[8..].copy_from_slice(&n.to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

fn push_model(args: &mut Vec<String>, model: Option<&str>) {
    let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    args.push("--model".to_string());
    args.push(model.to_string());
}

fn which_named(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let mut names = vec![name.to_string()];
    if cfg!(windows) && !name.ends_with(".exe") && !name.ends_with(".cmd") {
        names.push(format!("{name}.exe"));
        names.push(format!("{name}.cmd"));
    }
    for dir in std::env::split_paths(&path) {
        for candidate in &names {
            let file = dir.join(candidate);
            if file.is_file() {
                return Some(file);
            }
        }
    }
    None
}

fn read_version(program: &Path) -> Result<String, String> {
    let mut child = Command::new(program)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法读取 --version stdout".to_string())?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        let _ = tx.send(buf);
    });
    match rx.recv_timeout(VERSION_TIMEOUT) {
        Ok(buf) => {
            let _ = child.wait();
            if buf.trim().is_empty() {
                Err("空的版本输出".to_string())
            } else {
                Ok(buf)
            }
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err("版本探测超时".to_string())
        }
    }
}

fn first_line(raw: &str) -> String {
    raw.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(raw.trim())
        .to_string()
}

fn spawn(command: &EngineCommand) -> Result<String, String> {
    let mut child = Command::new(&command.program)
        .args(&command.args)
        .current_dir(&command.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "无法写入引擎 stdin".to_string())?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法读取引擎 stdout".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "无法读取引擎 stderr".to_string())?;

    stdin
        .write_all(command.stdin.as_bytes())
        .map_err(|error| error.to_string())?;
    drop(stdin);

    let (out_tx, out_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        let _ = out_tx.send(buf);
    });
    let (err_tx, err_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr.read_to_string(&mut buf);
        let _ = err_tx.send(buf);
    });

    let stdout = match out_rx.recv_timeout(TIMEOUT) {
        Ok(value) => value,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("引擎调用超时".to_string());
        }
    };
    let stderr = err_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or_default();
    let status = child.wait().map_err(|error| error.to_string())?;
    if status.success() {
        return Ok(stdout);
    }
    if !stderr.trim().is_empty() {
        return Err(stderr);
    }
    if !stdout.trim().is_empty() {
        return Err(stdout);
    }
    Err(format!("引擎退出码 {}", status.code().unwrap_or(-1)))
}

const MAP_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "summary": { "type": "string" }
  },
  "required": ["summary"],
  "additionalProperties": false
}"#;

const REDUCE_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "headline": { "type": "string" },
    "entries": {
      "type": "array",
      "minItems": 3,
      "maxItems": 6,
      "items": {
        "type": "object",
        "properties": {
          "title": { "type": "string" },
          "detail": { "type": "string" },
          "project": { "type": "string" }
        },
        "required": ["title", "detail", "project"],
        "additionalProperties": false
      }
    },
    "closing": { "type": "string" }
  },
  "required": ["headline", "entries", "closing"],
  "additionalProperties": false
}"#;
