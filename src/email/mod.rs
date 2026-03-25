pub mod templates;
pub mod sender;

pub use sender::{EmailMessage, EmailSender, start_email_worker};
