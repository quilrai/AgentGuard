use std::process::{Command, Stdio};
use std::time::Instant;

pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
}

pub fn run_command(command: &str, cwd: Option<&str>) -> CommandResult {
    let (shell, shell_flag) = shell_and_flag();
    let start = Instant::now();

    let mut cmd = Command::new(&shell);
    cmd.arg(&shell_flag)
        .arg(command)
        .env("LLMWATCHER_ACTIVE", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return CommandResult {
                stdout: String::new(),
                stderr: format!("llmwatcher: failed to execute: {e}"),
                exit_code: 127,
                duration_ms: start.elapsed().as_millis() as u64,
            };
        }
    };

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            return CommandResult {
                stdout: String::new(),
                stderr: format!("llmwatcher: failed to wait: {e}"),
                exit_code: 127,
                duration_ms: start.elapsed().as_millis() as u64,
            };
        }
    };

    CommandResult {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(1),
        duration_ms: start.elapsed().as_millis() as u64,
    }
}

fn shell_and_flag() -> (String, String) {
    let shell = detect_shell();
    let flag = if cfg!(windows) {
        let name = std::path::Path::new(&shell)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if name.contains("powershell") || name.contains("pwsh") {
            "-Command"
        } else if name == "cmd.exe" || name == "cmd" {
            "/C"
        } else {
            "-c"
        }
    } else {
        "-c"
    };
    (shell, flag.to_string())
}

fn detect_shell() -> String {
    if let Ok(shell) = std::env::var("LLMWATCHER_SHELL") {
        return shell;
    }

    if let Ok(shell) = std::env::var("SHELL") {
        return shell;
    }

    find_real_shell()
}

#[cfg(unix)]
fn find_real_shell() -> String {
    for shell in &["/bin/zsh", "/bin/bash", "/bin/sh"] {
        if std::path::Path::new(shell).exists() {
            return shell.to_string();
        }
    }
    "/bin/sh".to_string()
}

#[cfg(windows)]
fn find_real_shell() -> String {
    if let Ok(comspec) = std::env::var("COMSPEC") {
        return comspec;
    }
    "cmd.exe".to_string()
}
