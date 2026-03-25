use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    transport::smtp::authentication::Credentials,
};
use tokio::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;
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
    pub fn new() -> (Self, mpsc::Receiver<EmailMessage>) {
        let (tx, rx) = mpsc::channel(100);
        (Self { tx }, rx)
    }

    pub async fn send(&self, msg: EmailMessage) -> anyhow::Result<()> {
        self.tx.send(msg).await.map_err(|e| anyhow::anyhow!("Failed to queue email: {}", e))
    }
}

pub async fn start_email_worker(mut rx: mpsc::Receiver<EmailMessage>) {
    let host = std::env::var("SMTP_HOST").unwrap_or_else(|_| "localhost".into());
    let port = std::env::var("SMTP_PORT").unwrap_or_else(|_| "1025".into()).parse().unwrap_or(1025);
    let user = std::env::var("SMTP_USER").ok();
    let pass = std::env::var("SMTP_PASS").ok();
    let from = std::env::var("SMTP_FROM").unwrap_or_else(|_| "no-reply@stellar-tipjar.com".into());

    let mut mailer_builder = AsyncSmtpTransport::<Tokio1Executor>::relay(&host)
        .expect("SMTP host check failed")
        .port(port);

    if let (Some(u), Some(p)) = (user, pass) {
        mailer_builder = mailer_builder.credentials(Credentials::new(u, p));
    }

    let mailer = mailer_builder.build();

    tracing::info!("Email background worker started on {}:{}", host, port);

    while let Some(msg) = rx.recv().await {
        let body = match TERA.render(&msg.template_name, &msg.context) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("Failed to render template {}: {}", msg.template_name, e);
                continue;
            }
        };

        let email = match Message::builder()
            .from(from.parse().unwrap())
            .to(msg.to.parse().unwrap())
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

        // Attempt sending with retries
        let mut attempts = 0;
        let max_attempts = 3;
        loop {
            attempts += 1;
            match mailer.send(email.clone()).await {
                Ok(_) => {
                    tracing::debug!("Email sent successfully to {}", msg.to);
                    break;
                }
                Err(e) => {
                    tracing::error!("Failed to send email to {} (attempt {}/{}): {}", msg.to, attempts, max_attempts, e);
                    if attempts >= max_attempts {
                        tracing::error!("Giving up on email to {} after {} attempts", msg.to, max_attempts);
                        break;
                    }
                    tokio::time::sleep(Duration::from_secs(2u64.pow(attempts))).await; // Exponential backoff
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tera::Context;

    #[tokio::test]
    async fn test_email_queueing() {
        let (sender, mut rx) = EmailSender::new();
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
