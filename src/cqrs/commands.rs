use uuid::Uuid;

/// All write-side intents in the system.
#[derive(Debug)]
pub enum Command {
    RegisterCreator {
        username: String,
        wallet_address: String,
        email: Option<String>,
    },
    RecordTip {
        creator_username: String,
        amount: String,
        transaction_hash: String,
    },
}

/// The outcome of executing a command.
#[derive(Debug)]
pub enum CommandResult {
    CreatorRegistered { id: Uuid },
    TipRecorded { id: Uuid },
}
