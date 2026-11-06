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

exit codes: 0 both layers passed, 1 validation failure, 2 tool/configuration error";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Json,
    Text,
}

struct Args {
    input: PathBuf,
    structure_schema: PathBuf,
    profile: Option<PathBuf>,
    law_id: String,
    format: Format,
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
    let mut rest = args[1..].iter();
    while let Some(arg) = rest.next() {
        if arg == "-h" || arg == "--help" {
            return Ok(Parsed::Help);
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
    let law_id = law_id.unwrap_or_else(|| DEFAULT_LAW_ID.into());
    if law_id.is_empty() {
        return Err("--law-id must not be empty".into());
    }
    Ok(Parsed::Check(Args {
        input: input.ok_or("missing INPUT.md")?,
        structure_schema: structure_schema.ok_or("missing --structure-schema PATH")?,
        profile,
        law_id,
        format: format.unwrap_or(Format::Text),
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
            println!("{USAGE}");
            return;
        }
        Parsed::Version => {
            println!("loj-validator {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Parsed::Check(args) => args,
    };
    let options = CheckOptions {
        input: &args.input,
        structure_schema: &args.structure_schema,
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
