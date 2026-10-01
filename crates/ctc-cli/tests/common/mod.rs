use std::{fs, path::Path, process::Command as Process};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

pub struct Project {
    pub dir: TempDir,
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

impl Project {
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    pub fn write(&self, relative: &str, content: &str) {
        let path = self.dir.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    pub fn config(&self, value: &Value) {
        self.write(".ctc.json", &serde_json::to_string_pretty(value).unwrap());
    }

    pub fn no_try_template(&self) {
        self.write(
            ".ctmpl/no-try.ts.ctmpl",
            "{{ Forbidden | kind(\"TryStatement\") }}\n",
        );
    }

    pub fn run(&self, arguments: &[&str]) -> (i32, Value) {
        let output = Command::cargo_bin("ctc")
            .unwrap()
            .args(["--root", self.dir.path().to_str().unwrap()])
            .args(arguments)
            .args(["--format", "json"])
            .output()
            .unwrap();
        let report = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "not JSON ({error}): {}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
        (output.status.code().unwrap(), report)
    }

    pub fn run_subcommand(&self, subcommand: &str, arguments: &[&str]) -> (i32, Value) {
        let output = Command::cargo_bin("ctc")
            .unwrap()
            .arg(subcommand)
            .args(["--root", self.dir.path().to_str().unwrap()])
            .args(arguments)
            .args(["--format", "json"])
            .output()
            .unwrap();
        let report = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "not JSON ({error}): {}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
        (output.status.code().unwrap(), report)
    }

    pub fn git(&self, arguments: &[&str]) {
        git_in(self.dir.path(), arguments);
    }

    pub fn init_git(&self) {
        self.git(&["init", "--quiet", "--initial-branch=main"]);
        self.git(&["config", "user.email", "test@example.com"]);
        self.git(&["config", "user.name", "Test"]);
        self.git(&["config", "core.autocrlf", "false"]);
        self.git(&["config", "commit.gpgsign", "false"]);
    }

    pub fn commit_all(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "--quiet", "-m", message]);
    }
}

pub fn git_in(directory: &Path, arguments: &[&str]) {
    let output = Process::new("git")
        .arg("-C")
        .arg(directory)
        .args(["-c", "protocol.file.allow=always"])
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Code, rule identifier, path, and line of each diagnostic.
pub fn summary(report: &Value) -> Vec<(String, String, String, u64)> {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| {
            (
                diagnostic["code"].as_str().unwrap().to_string(),
                diagnostic["ruleId"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                diagnostic["source"]["path"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                diagnostic["source"]["start"]["line"]
                    .as_u64()
                    .unwrap_or_default(),
            )
        })
        .collect()
}

pub fn entry(code: &str, rule: &str, path: &str, line: u64) -> (String, String, String, u64) {
    (code.to_string(), rule.to_string(), path.to_string(), line)
}

pub fn guard(project: &Project, extra: &[&str]) -> (i32, Value) {
    let mut arguments = vec!["--base", "HEAD"];
    arguments.extend_from_slice(extra);
    project.run_subcommand("guard", &arguments)
}

pub fn lines(report: &Value) -> Vec<u64> {
    summary(report)
        .into_iter()
        .map(|(_, _, _, line)| line)
        .collect()
}

pub fn no_try_config(allow_ignore: bool) -> Value {
    serde_json::json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "no-try",
            "template": ".ctmpl/no-try.ts.ctmpl",
            "include": ["src/**/*.ts"],
            "mode": "forbid",
            "scope": "descendants",
            "allowIgnore": allow_ignore
        }]
    })
}

pub const TRY_WITH_IGNORE: &str = "export function run() {\n  // ctc-ignore-next-line no-try\n  try {\n    return 1;\n  } catch {\n    return 2;\n  }\n}\n";
