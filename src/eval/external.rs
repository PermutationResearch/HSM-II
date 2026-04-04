//! Subprocess "external benchmark" hook for TB2 / SWE-style graders (pass/fail + score).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::Stdio;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Declarative spec (e.g. from JSON) for spawning an external harness.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExternalBenchmarkSpec {
    pub name: String,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// Optional setup commands run before the main command (e.g. `cargo build --release`).
    #[serde(default)]
    pub setup_commands: Vec<Vec<String>>,
    pub command: Vec<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    /// Additional environment variables for setup + main commands.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Seconds (default 3600).
    #[serde(default = "default_timeout_sec")]
    pub timeout_sec: u64,
}

fn default_timeout_sec() -> u64 {
    3600
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExternalBenchmarkResult {
    pub name: String,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    pub exit_code: Option<i32>,
    pub passed: bool,
    /// True when `timeout_sec` elapsed and the process was killed.
    #[serde(default)]
    pub timed_out: bool,
    /// Normalized score in \[0,1\] — from JSON stdout if present, else derived from exit status.
    pub score: f64,
    #[serde(default)]
    pub setup_commands_run: usize,
    #[serde(default)]
    pub failed_phase: Option<String>,
    #[serde(default)]
    pub stdout_tail: String,
    #[serde(default)]
    pub stderr_tail: String,
}

#[derive(Deserialize)]
struct GraderStdout {
    #[serde(default)]
    pass: Option<bool>,
    #[serde(default)]
    passed: Option<bool>,
    #[serde(default)]
    score: Option<f64>,
}

fn tail(s: &str, max_bytes: usize) -> String {
    let b = s.as_bytes();
    if b.len() <= max_bytes {
        return s.to_string();
    }
    let start = b.len() - max_bytes;
    let slice = s
        .char_indices()
        .find(|(i, _)| *i >= start)
        .map(|(i, _)| i)
        .unwrap_or(start);
    format!("...{}", &s[slice..])
}

fn finish_result(
    spec: &ExternalBenchmarkSpec,
    stdout: String,
    stderr: String,
    exit: Option<i32>,
    success: bool,
    timed_out: bool,
    setup_commands_run: usize,
    failed_phase: Option<String>,
) -> ExternalBenchmarkResult {
    let (passed, score_from_json) = if timed_out {
        (false, None)
    } else {
        serde_json::from_str::<GraderStdout>(&stdout)
            .ok()
            .map(|g| {
                let p = g.pass.or(g.passed).unwrap_or(false);
                let sc = g.score.unwrap_or(if p { 1.0 } else { 0.0 });
                (p, Some(sc))
            })
            .unwrap_or((success, None))
    };

    let score = score_from_json.unwrap_or(if passed { 1.0 } else { 0.0 });

    ExternalBenchmarkResult {
        name: spec.name.clone(),
        labels: spec.labels.clone(),
        exit_code: exit,
        passed: passed && !timed_out,
        timed_out,
        score: if timed_out { 0.0 } else { score },
        setup_commands_run,
        failed_phase,
        stdout_tail: tail(&stdout, 8000),
        stderr_tail: tail(&stderr, 8000),
    }
}

struct CommandRun {
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
    success: bool,
    timed_out: bool,
}

fn spawn_command(
    command: &[String],
    cwd: Option<&PathBuf>,
    env: &BTreeMap<String, String>,
) -> std::io::Result<std::process::Child> {
    if command.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "external benchmark command is empty",
        ));
    }

    let mut cmd = std::process::Command::new(&command[0]);
    if command.len() > 1 {
        cmd.args(&command[1..]);
    }
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    if !env.is_empty() {
        cmd.envs(env);
    }
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.spawn()
}

fn run_command_with_timeout(
    command: &[String],
    cwd: Option<&PathBuf>,
    env: &BTreeMap<String, String>,
    timeout: Duration,
) -> std::io::Result<CommandRun> {
    let mut child = spawn_command(command, cwd, env)?;

    let mut stdout_pipe = child.stdout.take().expect("stdout piped");
    let mut stderr_pipe = child.stderr.take().expect("stderr piped");

    let th_out = thread::spawn(move || {
        let mut v = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut v);
        v
    });
    let th_err = thread::spawn(move || {
        let mut v = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut v);
        v
    });

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(status) => {
                let stdout = String::from_utf8_lossy(&th_out.join().unwrap_or_else(|_| Vec::new()))
                    .into_owned();
                let stderr = String::from_utf8_lossy(&th_err.join().unwrap_or_else(|_| Vec::new()))
                    .into_owned();
                return Ok(CommandRun {
                    stdout,
                    stderr,
                    exit_code: status.code(),
                    success: status.success(),
                    timed_out: false,
                });
            }
            None => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let stdout = String::from_utf8_lossy(
                        &th_out.join().unwrap_or_else(|_| Vec::new()),
                    )
                    .into_owned();
                    let stderr = String::from_utf8_lossy(
                        &th_err.join().unwrap_or_else(|_| Vec::new()),
                    )
                    .into_owned();
                    return Ok(CommandRun {
                        stdout,
                        stderr,
                        exit_code: None,
                        success: false,
                        timed_out: true,
                    });
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

fn append_phase_output(
    aggregate: &mut String,
    phase: &str,
    command: &[String],
    output: &str,
) {
    if output.trim().is_empty() {
        return;
    }
    aggregate.push_str(&format!(
        "\n== {} :: {} ==\n{}\n",
        phase,
        command.join(" "),
        output.trim()
    ));
}

/// Run external command; if stdout parses as JSON with `pass`/`passed`/`score`, use it.
/// Honors `timeout_sec` via `Child::try_wait` polling (pipes read on helper threads to avoid deadlock).
pub fn run_external_sync(spec: &ExternalBenchmarkSpec) -> std::io::Result<ExternalBenchmarkResult> {
    if spec.command.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "external benchmark command is empty",
        ));
    }
    let timeout = Duration::from_secs(spec.timeout_sec.max(1));
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut setup_commands_run = 0usize;

    for (idx, setup) in spec.setup_commands.iter().enumerate() {
        let run = run_command_with_timeout(setup, spec.cwd.as_ref(), &spec.env, timeout)?;
        append_phase_output(&mut stdout, &format!("setup[{idx}]"), setup, &run.stdout);
        append_phase_output(&mut stderr, &format!("setup[{idx}]"), setup, &run.stderr);
        if run.timed_out || !run.success {
            return Ok(finish_result(
                spec,
                stdout,
                stderr,
                run.exit_code,
                run.success,
                run.timed_out,
                setup_commands_run,
                Some(format!("setup[{idx}]")),
            ));
        }
        setup_commands_run += 1;
    };

    let run = run_command_with_timeout(&spec.command, spec.cwd.as_ref(), &spec.env, timeout)?;
    append_phase_output(&mut stdout, "main", &spec.command, &run.stdout);
    append_phase_output(&mut stderr, "main", &spec.command, &run.stderr);
    Ok(finish_result(
        spec,
        stdout,
        stderr,
        run.exit_code,
        run.success,
        run.timed_out,
        setup_commands_run,
        if run.success || run.timed_out {
            if run.timed_out {
                Some("main".to_string())
            } else {
                None
            }
        } else {
            Some("main".to_string())
        },
    ))
}

/// Multiple external graders (e.g. TB2 shards + a smoke check) in one JSON file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExternalBenchmarkBatch {
    pub benchmarks: Vec<ExternalBenchmarkSpec>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExternalBenchmarkBatchResult {
    pub results: Vec<ExternalBenchmarkResult>,
    pub mean_score: f64,
    pub all_passed: bool,
    /// Stopped early (`fail_fast`); later benchmarks were not run.
    #[serde(default)]
    pub stopped_early: bool,
}

/// Run each benchmark in order. With `fail_fast`, stops after the first failure (timeout counts as failure).
pub fn run_external_batch_sync(
    batch: &ExternalBenchmarkBatch,
    fail_fast: bool,
) -> std::io::Result<ExternalBenchmarkBatchResult> {
    let mut results = Vec::with_capacity(batch.benchmarks.len());
    let mut stopped_early = false;
    for spec in &batch.benchmarks {
        let r = run_external_sync(spec)?;
        let pass = r.passed;
        results.push(r);
        if fail_fast && !pass {
            stopped_early = true;
            break;
        }
    }
    let n = results.len().max(1);
    let mean_score: f64 = results.iter().map(|r| r.score).sum::<f64>() / n as f64;
    let all_passed = results.iter().all(|r| r.passed);
    Ok(ExternalBenchmarkBatchResult {
        results,
        mean_score,
        all_passed,
        stopped_early,
    })
}
