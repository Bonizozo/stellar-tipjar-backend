use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    transport::smtp::authentication::Credentials,
};
use tokio::sync::mpsc;
use std::time::Duration;
use crate::config::SmtpConfig;
use crate::email::templates::TERA;
use tera::Context;

#[derive(Debug, Clone)]
pub struct EmailMessage {
    pub to: String,
    pub subject: String,
    pub template_name: String,
    pub context: Context,
}

#[derive(Clone)]
pub struct EmailSender {
    tx: mpsc::Sender<EmailMessage>,
}

impl EmailSender {
    /// Create a sender/receiver pair.  The `cfg` is passed through to the worker
    /// via `start_email_worker_with_config`.
    pub fn new(_cfg: &SmtpConfig) -> (Self, mpsc::Receiver<EmailMessage>) {
        let (tx, rx) = mpsc::channel(100);
        (Self { tx }, rx)
    }

    pub async fn send(&self, msg: EmailMessage) -> anyhow::Result<()> {
        self.tx
            .send(msg)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to queue email: {}", e))
    }
}

/// Start the email background worker using values from [`SmtpConfig`].
///
/// This is the preferred entry point.  Configuration is read from `cfg` —
/// no `std::env::var` calls occur inside this function.
pub async fn start_email_worker_with_config(
    cfg: SmtpConfig,
    mut rx: mpsc::Receiver<EmailMessage>,
) {
    let mut mailer_builder = match AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = %e, smtp_host = %cfg.host, "Failed to create SMTP transport");
            return;
        }
    };
    mailer_builder = mailer_builder.port(cfg.port);

    if let (Some(user), Some(pass)) = (cfg.user, cfg.pass) {
        mailer_builder = mailer_builder.credentials(Credentials::new(user, pass));
    }

    let mailer = mailer_builder.build();
    let from_addr = cfg.from.clone();

    tracing::info!("Email background worker started on {}:{}", cfg.host, cfg.port);

    while let Some(msg) = rx.recv().await {
        let body = match TERA.render(&msg.template_name, &msg.context) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("Failed to render template {}: {}", msg.template_name, e);
                continue;
            }
        };

        let parsed_from = match from_addr.parse() {
            Ok(f) => f,
            Err(e) => {
                tracing::error!(error = %e, from = %from_addr, "Invalid SMTP from address");
                continue;
            }
        };
        let parsed_to = match msg.to.parse() {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(error = %e, to = %msg.to, "Invalid recipient address");
                continue;
            }
        };

        let email = match Message::builder()
            .from(parsed_from)
            .to(parsed_to)
            .subject(&msg.subject)
            .header(lettre::message::header::ContentType::TEXT_HTML)
            .body(body)
        {
            Ok(e) => e,
            Err(e) => {
                tracing::error!("Failed to build message: {}", e);
                continue;
            }
        };

        // Attempt sending with exponential-backoff retries.
        let mut attempts = 0u32;
        let max_attempts = 3u32;
        loop {
            attempts += 1;
            match mailer.send(email.clone()).await {
                Ok(_) => {
                    tracing::debug!("Email sent successfully to {}", msg.to);
                    break;
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to send email to {} (attempt {}/{}): {}",
                        msg.to,
                        attempts,
                        max_attempts,
                        e
                    );
                    if attempts >= max_attempts {
                        tracing::error!(
                            "Giving up on email to {} after {} attempts",
                            msg.to,
                            max_attempts
                        );
                        break;
                    }
                    tokio::time::sleep(Duration::from_secs(2u64.pow(attempts))).await;
                }
            }
        }
    }
}

/// Convenience wrapper — spawns the worker with the `SmtpConfig` captured from
/// a closure so that `main.rs` only needs to call `start_email_worker_with_config`.
///
/// # Note
/// This wrapper **does not** read from env; `cfg` comes from `AppConfig::smtp`.
pub async fn start_email_worker(cfg: SmtpConfig, rx: mpsc::Receiver<EmailMessage>) {
    start_email_worker_with_config(cfg, rx).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tera::Context;

    #[tokio::test]
    async fn test_email_queueing() {
        let cfg = SmtpConfig {
            host: "localhost".into(),
            port: 1025,
            user: None,
            pass: None,
            from: "no-reply@test.com".into(),
        };
        let (sender, mut rx) = EmailSender::new(&cfg);
        let mut context = Context::new();
        context.insert("username", "testuser");
        context.insert("amount", "10");
        context.insert("transaction_hash", "abc");

        let msg = EmailMessage {
            to: "test@example.com".into(),
            subject: "Test".into(),
            template_name: "tip_received.html".into(),
            context: context.clone(),
        };

        sender.send(msg).await.unwrap();
        let received = rx.recv().await.unwrap();

        assert_eq!(received.to, "test@example.com");
        assert_eq!(received.subject, "Test");
        assert_eq!(received.template_name, "tip_received.html");
    }
}
