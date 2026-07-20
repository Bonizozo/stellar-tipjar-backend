use tokio::signal;

pub async fn shutdown_signal() {
    let ctrl_c = async {
        // SAFETY: Installing a Ctrl-C handler fails only if the OS cannot register
        // the signal handler (e.g. process is being terminated or OS is out of
        // resources).  In either case the process cannot continue.
        // Invariant: ctrl_c() installation always succeeds on supported platforms.
        #[allow(clippy::expect_used)]
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        // SAFETY: Same reasoning as Ctrl-C — SIGTERM registration only fails
        // under abnormal OS conditions where the process cannot run anyway.
        // Invariant: SIGTERM handler installation always succeeds on Unix.
        #[allow(clippy::expect_used)]
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received, draining in-flight requests...");
}
