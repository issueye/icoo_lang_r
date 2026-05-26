use std::fmt;
use std::sync::Arc;

type RuntimeLogCallback = dyn Fn(RuntimeLogRecord) + Send + Sync + 'static;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Debug => write!(f, "debug"),
            Self::Info => write!(f, "info"),
            Self::Warn => write!(f, "warn"),
            Self::Error => write!(f, "error"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLogRecord {
    pub level: LogLevel,
    pub target: String,
    pub message: String,
}

impl RuntimeLogRecord {
    pub fn new(level: LogLevel, target: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level,
            target: target.into(),
            message: message.into(),
        }
    }
}

#[derive(Clone, Default)]
pub struct RuntimeLogger {
    callback: Option<Arc<RuntimeLogCallback>>,
}

impl RuntimeLogger {
    pub fn noop() -> Self {
        Self { callback: None }
    }

    pub fn callback<F>(callback: F) -> Self
    where
        F: Fn(RuntimeLogRecord) + Send + Sync + 'static,
    {
        Self {
            callback: Some(Arc::new(callback)),
        }
    }

    pub fn log(&self, record: RuntimeLogRecord) {
        if let Some(callback) = &self.callback {
            callback(record);
        }
    }

    pub fn log_message(
        &self,
        level: LogLevel,
        target: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.log(RuntimeLogRecord::new(level, target, message));
    }

    pub fn is_noop(&self) -> bool {
        self.callback.is_none()
    }
}

impl fmt::Debug for RuntimeLogger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeLogger")
            .field("is_noop", &self.is_noop())
            .finish()
    }
}
