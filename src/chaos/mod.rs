pub mod experiments;
pub mod injectors;
pub mod metrics;
pub mod scenarios;

#[derive(Debug, thiserror::Error)]
pub enum ChaosError {
    #[error("injected failure: {0}")]
    InjectedFailure(String),
    #[error("experiment setup error: {0}")]
    Setup(String),
}
