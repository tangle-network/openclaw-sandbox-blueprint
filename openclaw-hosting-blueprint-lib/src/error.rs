/// Errors produced by OpenClaw hosting operations.
#[derive(Debug)]
pub enum HostingError {
    /// The requested instance was not found.
    InstanceNotFound(String),
    /// The requested template pack was not found.
    TemplateNotFound(String),
    /// An instance lifecycle transition is not valid from the current state.
    InvalidStateTransition {
        instance_id: String,
        current: String,
        attempted: String,
    },
    /// Persistence layer failure.
    Store(String),
    /// Serialization or deserialization failure.
    Serde(String),
    /// I/O error.
    Io(String),
}

impl std::fmt::Display for HostingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InstanceNotFound(id) => write!(f, "instance not found: {id}"),
            Self::TemplateNotFound(id) => write!(f, "template pack not found: {id}"),
            Self::InvalidStateTransition {
                instance_id,
                current,
                attempted,
            } => write!(
                f,
                "invalid state transition for {instance_id}: {current} -> {attempted}"
            ),
            Self::Store(msg) => write!(f, "store error: {msg}"),
            Self::Serde(msg) => write!(f, "serde error: {msg}"),
            Self::Io(msg) => write!(f, "io error: {msg}"),
        }
    }
}

impl std::error::Error for HostingError {}

impl From<serde_json::Error> for HostingError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serde(err.to_string())
    }
}

impl From<std::io::Error> for HostingError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.to_string())
    }
}

/// Convert into the `String` error type expected by Tangle job handlers.
impl From<HostingError> for String {
    fn from(err: HostingError) -> Self {
        err.to_string()
    }
}

pub type Result<T> = std::result::Result<T, HostingError>;
