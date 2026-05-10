use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::time::Instant;

pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
}

pub fn run_command(
    command: &str,
    cwd: Option<&str>,
    shell_override: Option<&str>,
    env: Option<&HashMap<String, String>>,
) -> CommandResult {
    let (shell, shell_flag) = shell_and_flag(shell_override);
    let start = Instant::now();

    let mut cmd = Command::new(&shell);
    cmd.arg(&shell_flag)
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    if let Some(env) = env {
        cmd.envs(env);
    }
    cmd.env("LLMWATCHER_ACTIVE", "1");

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

    let stdout = match String::from_utf8(output.stdout) {
        Ok(s) => s,
        Err(e) => {
            let bytes = e.into_bytes();
            String::from_utf8_lossy(&bytes).into_owned()
        }
    };
    let stderr = match String::from_utf8(output.stderr) {
        Ok(s) => s,
        Err(e) => {
            let bytes = e.into_bytes();
            String::from_utf8_lossy(&bytes).into_owned()
        }
    };

    CommandResult {
        stdout,
        stderr,
        exit_code: output.status.code().unwrap_or(1),
        duration_ms: start.elapsed().as_millis() as u64,
    }
}

fn shell_and_flag(shell_override: Option<&str>) -> (String, String) {
    let shell = detect_shell(shell_override);
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

fn detect_shell(shell_override: Option<&str>) -> String {
    if let Some(shell) = shell_override.filter(|s| !s.trim().is_empty()) {
        return shell.to_string();
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    #[test]
    fn run_command_honors_shell_override() {
        let result = run_command("printf '%s' \"$0\"", None, Some("/bin/sh"), None);
        assert_eq!(result.exit_code, 0);
        assert!(
            result.stdout.contains("sh"),
            "expected shell override to be reflected in $0, got {:?}",
            result.stdout
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_command_uses_forwarded_path_env() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("llmwatcher_env_path_{nonce}"));
        fs::create_dir_all(&dir).unwrap();
        let tool = dir.join("agent-path-tool");
        fs::write(&tool, "#!/bin/sh\nprintf forwarded-path\n").unwrap();

        let mut perms = fs::metadata(&tool).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&tool, perms).unwrap();

        let mut env = HashMap::new();
        env.insert("PATH".to_string(), dir.to_string_lossy().to_string());

        let result = run_command("agent-path-tool", None, Some("/bin/sh"), Some(&env));
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "forwarded-path");

        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn run_command_forces_active_marker_after_forwarded_env() {
        let mut env = HashMap::new();
        env.insert("LLMWATCHER_ACTIVE".to_string(), "0".to_string());

        let result = run_command(
            "printf '%s' \"$LLMWATCHER_ACTIVE\"",
            None,
            Some("/bin/sh"),
            Some(&env),
        );
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1");
    }
}
