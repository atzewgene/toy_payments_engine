/// Top-level errors in the main application flow that should exit the program and be reported to the user.
#[derive(thiserror::Error, Debug, Clone, Copy)]
pub struct AppError;

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AppError")
    }
}
