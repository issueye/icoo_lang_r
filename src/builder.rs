use crate::error::IcooResult;
use crate::interpreter::Interpreter;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::http_config::RuntimeHttpConfig;
use crate::runtime::logging::RuntimeLogger;
use crate::runtime::permissions::RuntimePermissions;
use crate::runtime::value::Value;
use std::path::Path;
use std::time::Duration;

pub struct InterpreterBuilder {
    output: Option<Box<dyn FnMut(String) + 'static>>,
    permissions: RuntimePermissions,
    logger: RuntimeLogger,
    config: RuntimeConfig,
    http_config: RuntimeHttpConfig,
    http_tls_roots: Option<std::sync::Arc<rustls::RootCertStore>>,
}

impl InterpreterBuilder {
    pub fn new() -> Self {
        Self {
            output: None,
            permissions: RuntimePermissions::default(),
            logger: RuntimeLogger::default(),
            config: RuntimeConfig::default(),
            http_config: RuntimeHttpConfig::default(),
            http_tls_roots: None,
        }
    }

    pub fn output<F: FnMut(String) + 'static>(mut self, output: F) -> Self {
        self.output = Some(Box::new(output));
        self
    }

    pub fn permissions(mut self, permissions: RuntimePermissions) -> Self {
        self.permissions = permissions;
        self
    }

    pub fn timeout(mut self, duration: Duration) -> Self {
        self.config.exec_timeout = Some(duration);
        self
    }

    pub fn max_memory(mut self, bytes: usize) -> Self {
        self.config.max_memory = bytes;
        self
    }

    pub fn max_call_depth(mut self, depth: usize) -> Self {
        self.config.max_call_depth = depth;
        self
    }

    pub fn http_timeout(mut self, timeout: Duration) -> Self {
        self.http_config = self
            .http_config
            .with_timeouts(timeout, timeout, timeout);
        self
    }

    pub fn http_max_redirects(mut self, max_redirects: usize) -> Self {
        self.http_config = self.http_config.with_max_redirects(max_redirects);
        self
    }

    pub fn http_tls_roots(mut self, roots: rustls::RootCertStore) -> Self {
        self.http_tls_roots = Some(std::sync::Arc::new(roots));
        self
    }

    pub fn build(self) -> Interpreter {
        let output = self
            .output
            .unwrap_or_else(|| Box::new(|line| println!("{}", line)));
        Interpreter::with_output_permissions_logger_tls_roots_and_http_config(
            output,
            self.permissions,
            self.logger,
            self.http_tls_roots,
            self.http_config,
            self.config,
        )
    }

    pub fn run_source(self, source: &str) -> IcooResult<Value> {
        let program = crate::parse_and_check(source)?;
        let mut interpreter = self.build();
        interpreter.interpret(&program)
    }

    pub fn run_file(self, path: impl AsRef<Path>) -> IcooResult<Value> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path).map_err(|err| {
            crate::error::IcooError::runtime(
                format!("failed to read file '{}': {}", path.display(), err),
                None,
            )
        })?;
        self.run_source(&source)
    }
}

impl Default for InterpreterBuilder {
    fn default() -> Self {
        Self::new()
    }
}
