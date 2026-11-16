//! Real Unix signals must unwind bundle ownership, including an exited leader.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn signals_stop_git_descendants_and_remove_private_directories() {
    for signal in ["-INT", "-TERM"] {
        for ending in ["wait", "exit 0"] {
            let root = std::env::temp_dir().join(format!(
                "loj-interrupt-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&root).unwrap();
            let bin = root.join("bin");
            let temp = root.join("temp");
            fs::create_dir(&bin).unwrap();
            fs::create_dir(&temp).unwrap();
            let ready = root.join("ready");
            let survived = root.join("survived");
            let git = bin.join("git");
            fs::write(&git, format!("#!/bin/sh\n(printf ready > \"$LOJ_READY\"; sleep 2; printf survived > \"$LOJ_SURVIVED\") &\n{ending}\n")).unwrap();
            fs::set_permissions(&git, fs::Permissions::from_mode(0o700)).unwrap();
            let bundle = root.join("input.bundle");
            fs::write(&bundle, b"synthetic input").unwrap();
            let mut paths = vec![bin];
            paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
            let mut child = Command::new(env!("CARGO_BIN_EXE_loj-validator"))
                .args([
                    "check",
                    bundle.to_str().unwrap(),
                    "--base",
                    "aaaa",
                    "--head",
                    "bbbb",
                    "--format",
                    "json",
                ])
                .env("PATH", std::env::join_paths(paths).unwrap())
                .env("TMPDIR", &temp)
                .env("LOJ_READY", &ready)
                .env("LOJ_SURVIVED", &survived)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let start = Instant::now();
            while !ready.exists() && start.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(10));
            }
            if !ready.exists() {
                let _ = child.kill();
                let output = child.wait_with_output().unwrap();
                panic!("git did not start: {output:?}");
            }
            assert!(fs::read_dir(&temp).unwrap().next().is_some());
            assert!(Command::new("kill")
                .args([signal, &child.id().to_string()])
                .status()
                .unwrap()
                .success());
            let start = Instant::now();
            loop {
                if child.try_wait().unwrap().is_some() {
                    break;
                }
                if start.elapsed() > Duration::from_secs(10) {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("interrupted CLI did not finish");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let output = child.wait_with_output().unwrap();
            assert_eq!(output.status.code(), Some(3), "{output:?}");
            let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(report["error"]["kind"], "incomplete");
            assert!(fs::read_dir(&temp).unwrap().next().is_none());
            std::thread::sleep(Duration::from_millis(2200));
            assert!(
                !survived.exists(),
                "git descendant survived {signal} ({ending})"
            );
            fs::remove_dir_all(root).unwrap();
        }
    }
}
