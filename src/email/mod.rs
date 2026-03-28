pub mod sender;
pub mod templates;

pub use sender::{start_email_worker, EmailMessage, EmailSender};
