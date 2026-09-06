use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[cfg(test)]
use std::collections::VecDeque;
#[cfg(test)]
use std::sync::Mutex;

use crate::domain::{EngineCommand, EngineProfile};

const TIMEOUT: Duration = Duration::from_secs(180);
const WORK_DIR_NAME: &str = "work-notes-engine";

pub fn codex_profile() -> EngineProfile {
    EngineProfile {
        id: "codex".to_string(),
        program: "codex".to_string(),
        writes_session_dir: false,
    }
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

pub fn codex_command(
    work_dir: &Path,
    schema: SchemaKind,
    stdin: String,
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
    Ok(EngineCommand {
        program: codex_profile().program,
        args: vec![
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
            "-".to_string(),
        ],
        stdin,
        cwd: work_dir.to_path_buf(),
    })
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
