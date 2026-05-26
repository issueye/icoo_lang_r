use std::time::Duration;

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub exec_timeout: Option<Duration>,
    pub max_call_depth: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            exec_timeout: None,
            max_call_depth: crate::runtime::limits::MAX_CALL_DEPTH,
        }
    }
}

impl RuntimeConfig {
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.exec_timeout = Some(timeout);
        self
    }

    pub fn with_max_call_depth(mut self, depth: usize) -> Self {
        self.max_call_depth = depth;
        self
    }
}
