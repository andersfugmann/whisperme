pub mod audio;
pub mod audio_processor;
pub mod config;
pub mod fanout;
pub mod injection;
pub mod socket;
pub mod transcription;
pub mod ui;

/// Extension trait for fail-fast error handling.
///
/// All operations use `.unwrap_or_exit()` which:
/// - Prints error message to stderr
/// - Calls `exit(1)`
/// - Terminates entire process immediately
pub trait UnwrapOrExit<T> {
    /// Unwraps the value or exits with an error message.
    fn unwrap_or_exit(self, msg: &str) -> T;
}

impl<T, E: std::fmt::Display> UnwrapOrExit<T> for Result<T, E> {
    fn unwrap_or_exit(self, msg: &str) -> T {
        match self {
            Ok(v) => v,
            Err(e) => {
                eprintln!("error: {msg}: {e}");
                std::process::exit(1);
            }
        }
    }
}

impl<T> UnwrapOrExit<T> for Option<T> {
    fn unwrap_or_exit(self, msg: &str) -> T {
        match self {
            Some(v) => v,
            None => {
                eprintln!("error: {msg}");
                std::process::exit(1);
            }
        }
    }
}
