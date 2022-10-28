pub type ActorResult<T> = std::result::Result<T, ActorError>;

#[derive(Debug)]
pub enum ActorError {
    MailboxClosed(&'static str),
    ActorPanic(&'static str),
}

impl std::error::Error for ActorError {}

impl std::fmt::Display for ActorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActorError::MailboxClosed(kind) => write!(f, "Address<{}> is closed", kind),
            ActorError::ActorPanic(kind) => write!(f, "Address<{}> is paniced while closing", kind),
        }
    }
}
