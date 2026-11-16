//! Signal registration belongs to the executable, never to the library.
#[cfg(unix)]
pub fn execute(args: &loj_validator::next::Args) -> (String, i32) {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let flag = Arc::new(AtomicBool::new(false));
    // Keep registrations through output and process exit, so a second signal
    // cannot terminate cleanup or truncate the report.
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        if let Err(error) = signal_hook::flag::register(signal, flag.clone()) {
            return incomplete(
                args.json,
                &format!("cannot install signal handler: {error}"),
            );
        }
    }
    let result =
        law_semantic::cancellation::with_flag(flag.clone(), || loj_validator::next::execute(args));
    // This is the completion boundary. Signals after it do not change output.
    if flag.load(Ordering::SeqCst) {
        incomplete(args.json, "validation interrupted")
    } else {
        result
    }
}

#[cfg(unix)]
fn incomplete(json: bool, message: &str) -> (String, i32) {
    (
        if json {
            serde_json::json!({"error": {"kind": "incomplete", "message": message}}).to_string()
        } else {
            format!("incomplete: {message}")
        },
        3,
    )
}

#[cfg(not(unix))]
pub fn execute(args: &loj_validator::next::Args) -> (String, i32) {
    loj_validator::next::execute(args)
}
