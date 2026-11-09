use loj_validator::{check, error_report, render_text, CheckOptions, Report, DEFAULT_LAW_ID};
use std::io::Write;
use std::path::PathBuf;
use std::process;

const USAGE: &str = "usage: loj-validator check INPUT.md --structure-schema PATH \
[--profile PATH] [--law-id ID] [--format json|text]

  --structure-schema PATH  schematter document schema (YAML/JSON), required
  --profile PATH           law-semantic mapping profile (default: embedded law-of-japan)
  --law-id ID              semantic law identity (default: \"law\")
  --format json|text       output format (default: text)

v1alpha1: loj-validator check DIR|law.yaml --profile v1alpha1 [--level L0|L1]
          [--format json|text] [--strict]
Legacy default profile may also be selected with --profile legacy.
Without --profile, --structure-schema or a .md input, check uses the next
specification (loj/v1alpha2); see below.
exit codes: 0 passed, 1 validation failure, 2 tool/configuration error, 3 incomplete";

/// Inputs that keep the legacy or v1alpha1 pipelines; everything else is the next spec.
fn uses_next_spec(args: &[String]) -> bool {
    args.first().map(String::as_str) == Some("check")
        && !args[1..].iter().any(|a| {
            a.starts_with("--profile")
                || a.starts_with("--structure-schema")
                || a.starts_with("--law-id")
                || a == "--strict"
                || a == "-h"
                || a == "--help"
                || (!a.starts_with("--") && a.ends_with(".md"))
        })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Json,
    Text,
}

struct Args {
    input: PathBuf,
    structure_schema: Option<PathBuf>,
    profile: Option<PathBuf>,
    law_id: String,
    format: Format,
    level: law_semantic::v1alpha1::Level,
    v1alpha1: bool,
}

enum Parsed {
    Check(Args),
    Help,
    Version,
}

/// Best effort before full parsing, so usage errors can still be JSON.
fn requested_format(args: &[String]) -> Format {
    for (i, a) in args.iter().enumerate() {
        if a == "--format=json"
            || (a == "--format" && args.get(i + 1).map(String::as_str) == Some("json"))
        {
            return Format::Json;
        }
    }
    Format::Text
}

fn parse(args: &[String]) -> Result<Parsed, String> {
    match args.first().map(String::as_str) {
        None => return Err("missing command".into()),
        Some("-h" | "--help" | "help") => return Ok(Parsed::Help),
        Some("-V" | "--version") => return Ok(Parsed::Version),
        Some("check") => {}
        Some(other) => return Err(format!("unknown command: {other}")),
    }
    let mut input = None;
    let mut structure_schema = None;
    let mut profile = None;
    let mut law_id = None;
    let mut format = None;
    let mut level = None;
    let mut strict = false;
    let mut rest = args[1..].iter();
    while let Some(arg) = rest.next() {
        if arg == "-h" || arg == "--help" {
            return Ok(Parsed::Help);
        }
        if arg == "--strict" {
            if strict {
                return Err("--strict given more than once".into());
            }
            strict = true;
            continue;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        if !name.starts_with("--") {
            if input.replace(PathBuf::from(arg)).is_some() {
                return Err(format!("unexpected argument: {arg}"));
            }
            continue;
        }
        let value = match inline {
            Some(v) => v,
            None => rest
                .next()
                .cloned()
                .ok_or_else(|| format!("{name} requires a value"))?,
        };
        let slot_taken = match name {
            "--structure-schema" => structure_schema.replace(PathBuf::from(&value)).is_some(),
            "--profile" => profile.replace(PathBuf::from(&value)).is_some(),
            "--law-id" => law_id.replace(value.clone()).is_some(),
            "--level" => {
                let value = match value.as_str() {
                    "L0" => law_semantic::v1alpha1::Level::L0,
                    "L1" => law_semantic::v1alpha1::Level::L1,
                    _ => return Err("--level must be L0 or L1".into()),
                };
                level.replace(value).is_some()
            }
            "--format" => {
                let f = match value.as_str() {
                    "json" => Format::Json,
                    "text" => Format::Text,
                    _ => {
                        return Err(format!(
                            "unsupported format: {value} (expected json or text)"
                        ))
                    }
                };
                format.replace(f).is_some()
            }
            _ => return Err(format!("unknown option: {name}")),
        };
        if slot_taken {
            return Err(format!("{name} given more than once"));
        }
    }
    let input = input.ok_or("missing INPUT.md or repository")?;
    let profile_name = profile.as_deref().and_then(|p| p.to_str());
    let v1alpha1 = profile_name == Some("v1alpha1");
    if v1alpha1 && (structure_schema.is_some() || law_id.is_some()) {
        return Err("v1alpha1 gets schema and law identity from law.yaml".into());
    }
    if !v1alpha1 && (level.is_some() || strict) {
        return Err("--level and --strict require v1alpha1".into());
    }
    if !v1alpha1 && structure_schema.is_none() {
        return Err("missing --structure-schema PATH".into());
    }
    if matches!(profile_name, Some("legacy" | "v1alpha1")) {
        profile = None;
    }
    let law_id = law_id.unwrap_or_else(|| DEFAULT_LAW_ID.into());
    if law_id.is_empty() {
        return Err("--law-id must not be empty".into());
    }
    Ok(Parsed::Check(Args {
        input,
        structure_schema,
        profile,
        law_id,
        format: format.unwrap_or(Format::Text),
        level: level.unwrap_or(law_semantic::v1alpha1::Level::L1),
        v1alpha1,
    }))
}

fn emit(report: &Report, format: Format) -> i32 {
    let text = match format {
        Format::Json => serde_json::to_string_pretty(&report.json).unwrap_or_else(|e| {
            format!("{{\"status\":\"error\",\"exit_code\":2,\"errors\":[{{\"kind\":\"internal\",\"message\":\"{e}\"}}]}}")
        }),
        Format::Text => render_text(report),
    };
    let mut stdout = std::io::stdout().lock();
    if writeln!(stdout, "{}", text.trim_end()).is_err() {
        return 2;
    }
    report.exit_code()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let format = requested_format(&args);
    if uses_next_spec(&args) {
        match loj_validator::next::parse(&args[1..]) {
            Ok(a) => {
                let (text, code) = std::panic::catch_unwind(|| loj_validator::next::execute(&a))
                    .unwrap_or_else(|_| ("error: validator panicked; this is a bug".into(), 2));
                if writeln!(std::io::stdout().lock(), "{}", text.trim_end()).is_err() {
                    process::exit(2);
                }
                process::exit(code);
            }
            Err(message) => {
                if format == Format::Json {
                    process::exit(emit(&error_report("usage", &message, None), format));
                }
                eprintln!(
                    "error: {message}

{USAGE}

{}",
                    loj_validator::next::USAGE
                );
                process::exit(2);
            }
        }
    }
    let parsed = match parse(&args) {
        Ok(p) => p,
        Err(message) => {
            if format == Format::Json {
                process::exit(emit(&error_report("usage", &message, None), format));
            }
            eprintln!("error: {message}\n\n{USAGE}");
            process::exit(2);
        }
    };
    let args = match parsed {
        Parsed::Help => {
            println!(
                "{USAGE}

{}",
                loj_validator::next::USAGE
            );
            return;
        }
        Parsed::Version => {
            println!("loj-validator {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Parsed::Check(args) => args,
    };
    if args.v1alpha1 {
        let report = std::panic::catch_unwind(|| law_semantic::v1alpha1::check(&args.input, args.level))
            .unwrap_or_else(|_| serde_json::json!({"status":"Error","exit_code":2,"complete":false,"diagnostics":[{"code":"internal","message":"validator panicked; this is a bug"}]}));
        let output = if args.format == Format::Json {
            serde_json::to_string_pretty(&report).unwrap()
        } else {
            let mut s = format!(
                "loj/v1alpha1 {:?}: {}\n",
                args.level,
                report["status"].as_str().unwrap()
            );
            if let Some(ds) = report["diagnostics"].as_array() {
                for d in ds {
                    s.push_str(&format!(
                        "{} [{}..{}] {}: {}\n",
                        d["file"], d["span"]["start"], d["span"]["end"], d["rule"], d["message"]
                    ));
                }
            }
            s
        };
        let code = if writeln!(std::io::stdout().lock(), "{}", output.trim_end()).is_err() {
            2
        } else {
            report["exit_code"].as_i64().unwrap_or(2) as i32
        };
        process::exit(code);
    }
    let options = CheckOptions {
        input: &args.input,
        structure_schema: args
            .structure_schema
            .as_deref()
            .expect("legacy requires schema"),
        profile: args.profile.as_deref(),
        law_id: &args.law_id,
    };
    // Last line of defence: an unexpected panic still yields a parseable report.
    let report = std::panic::catch_unwind(|| check(&options)).unwrap_or_else(|_| {
        error_report(
            "internal",
            "validator panicked; this is a bug",
            Some(&args.input.to_string_lossy()),
        )
    });
    process::exit(emit(&report, args.format));
}
