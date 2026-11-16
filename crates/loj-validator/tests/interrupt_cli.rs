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

/// Numbered articles of a synthetic package (kanji numerals up to 9999).
fn synthetic_text(articles: usize) -> String {
    const DIGITS: [&str; 10] = ["", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    let kanji = |mut n: usize| {
        let mut out = String::new();
        for (unit, name) in [(1000, "千"), (100, "百"), (10, "十")] {
            let d = n / unit;
            if d > 0 {
                out.push_str(if d > 1 { DIGITS[d] } else { "" });
                out.push_str(name);
            }
            n %= unit;
        }
        out + DIGITS[n]
    };
    (1..=articles)
        .map(|i| {
            format!(
                "# 第{}条\n<!-- loj:paragraph number=1 -->\n本文。\n<!-- loj:end-paragraph -->\n\n",
                kanji(i)
            )
        })
        .collect()
}

/// A repository whose package checks take far longer than the response
/// bound, and which never calls git: cancellation must be observed between
/// and within Markdown checks, not only around git.
#[test]
fn signals_interrupt_large_checks_without_git() {
    for signal in ["-INT", "-TERM"] {
        let root = std::env::temp_dir().join(format!(
            "loj-interrupt-nogit-{}-{}",
            std::process::id(),
            &signal[1..]
        ));
        let _ = fs::remove_dir_all(&root);
        // Removes the synthetic input even when an assertion fails.
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let repo = root.join("repo");
        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let called = root.join("git-called");
        let git = bin.join("git");
        fs::write(&git, "#!/bin/sh\nprintf called > \"$LOJ_CALLED\"\nexit 1\n").unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o700)).unwrap();
        let revisions = 200;
        let ids: Vec<String> = (0..revisions).map(|i| format!("r{i:07}")).collect();
        let history = serde_json::json!({"schemaVersion":"loj-history/v1alpha1","record":{"id":"synthetic"},
            "revisions": ids.iter().map(|id| serde_json::json!({"id":id,"text":{"status":"complete"}})).collect::<Vec<_>>()});
        fs::create_dir_all(&repo).unwrap();
        fs::write(repo.join("history.yaml"), history.to_string()).unwrap();
        let manifest = serde_json::json!({"schemaVersion":"loj/v1alpha2","law":{"id":"synthetic","title":"Synthetic law","type":"act","number":null,"promulgatedAt":null,"language":"ja"},"documents":[{"path":"main.md","parent":"main"}]});
        let text = synthetic_text(2000);
        for id in &ids {
            let dir = repo.join("versions").join(id);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("law.yaml"), manifest.to_string()).unwrap();
            fs::write(dir.join("main.md"), &text).unwrap();
        }
        let mut child = Command::new(env!("CARGO_BIN_EXE_loj-validator"))
            .args([
                "check",
                repo.to_str().unwrap(),
                "--check",
                "L1",
                "--format",
                "json",
            ])
            .env("PATH", bin.as_os_str())
            .env("LOJ_CALLED", &called)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Signal handlers are installed before any input is read.
        std::thread::sleep(Duration::from_millis(1000));
        if child.try_wait().unwrap().is_some() {
            let output = child.wait_with_output().unwrap();
            panic!("the synthetic input finished before the signal: {output:?}");
        }
        let sent = Command::new("kill")
            .args([signal, &child.id().to_string()])
            .status()
            .is_ok_and(|status| status.success());
        if !sent {
            let _ = child.kill();
            let _ = child.wait();
            panic!("cannot send {signal}");
        }
        let start = Instant::now();
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if start.elapsed() > Duration::from_secs(10) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("interrupted CLI did not stop at a checkpoint ({signal})");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(3), "{output:?}");
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["error"]["kind"], "incomplete");
        assert_eq!(report["error"]["message"], "validation interrupted");
        assert!(!called.exists(), "git was called");
    }
}
