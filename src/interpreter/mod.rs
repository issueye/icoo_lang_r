use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use crate::parser::ast::Program;
use crate::runtime::env::{BindingKind, EnvRef, Environment};
use crate::runtime::http_config::RuntimeHttpConfig;
use crate::runtime::logging::RuntimeLogger;
use crate::runtime::permissions::RuntimePermissions;
use crate::runtime::value::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

mod args;
mod calls;
mod classes;
mod coroutines;
mod eval;
mod formats;
mod http_alpn;
mod http_client;
mod http_common;
mod http_proxy;
mod http_redirect;
mod http_server;
mod http_url;
mod methods;
mod modules;
mod tasks;
mod types;
mod web_ino;

pub(crate) use args::{
    arity_error, clamp_slice_index, expect_arity, expect_byte_index, expect_bytes, expect_int,
    expect_number, expect_string, normalize_index, now_duration, numeric_min_max,
};
pub(crate) use formats::{json_to_value, toml_to_value, value_to_json, value_to_toml};
pub(crate) use http_client::{
    http_client_request, http_client_request_bytes, http_stream_method_name, HttpClientHeaders,
};
pub(crate) use http_server::http_server_serve_once;
pub(crate) use types::{check_value_type, is_callable, value_equal};

pub struct Interpreter {
    env: EnvRef,
    output: Box<dyn FnMut(String)>,
    current_loop: Option<Rc<RefCell<IcooEventLoop>>>,
    current_task: Option<Rc<RefCell<IcooTask>>>,
    module_cache: HashMap<PathBuf, Rc<IcooModule>>,
    loading_modules: Vec<PathBuf>,
    current_module_dir: Option<PathBuf>,
    permissions: RuntimePermissions,
    logger: RuntimeLogger,
    http_config: RuntimeHttpConfig,
    http_tls_roots: Option<Arc<rustls::RootCertStore>>,
    http_tls_config: RefCell<Option<Arc<rustls::ClientConfig>>>,
    call_depth: usize,
    runtime_config: crate::runtime::config::RuntimeConfig,
    execution_deadline: Option<std::time::Instant>,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    pub fn new() -> Self {
        Self::with_output(|line| println!("{}", line))
    }

    pub fn with_output<F>(output: F) -> Self
    where
        F: FnMut(String) + 'static,
    {
        Self::with_output_and_permissions(output, RuntimePermissions::default())
    }

    pub fn with_permissions(permissions: RuntimePermissions) -> Self {
        Self::with_output_and_permissions(|line| println!("{}", line), permissions)
    }

    pub fn with_logger(logger: RuntimeLogger) -> Self {
        Self::with_output_permissions_and_logger(
            |line| println!("{}", line),
            RuntimePermissions::default(),
            logger,
        )
    }

    pub fn with_output_and_permissions<F>(output: F, permissions: RuntimePermissions) -> Self
    where
        F: FnMut(String) + 'static,
    {
        Self::with_output_permissions_and_logger(output, permissions, RuntimeLogger::default())
    }

    pub fn with_output_permissions_and_logger<F>(
        output: F,
        permissions: RuntimePermissions,
        logger: RuntimeLogger,
    ) -> Self
    where
        F: FnMut(String) + 'static,
    {
        Self::with_output_permissions_logger_tls_roots_and_http_config(
            output,
            permissions,
            logger,
            None,
            RuntimeHttpConfig::default(),
            crate::runtime::config::RuntimeConfig::default(),
        )
    }

    pub fn with_output_permissions_logger_and_tls_roots<F>(
        output: F,
        permissions: RuntimePermissions,
        logger: RuntimeLogger,
        http_tls_roots: Option<Arc<rustls::RootCertStore>>,
    ) -> Self
    where
        F: FnMut(String) + 'static,
    {
        Self::with_output_permissions_logger_tls_roots_and_http_config(
            output,
            permissions,
            logger,
            http_tls_roots,
            RuntimeHttpConfig::default(),
            crate::runtime::config::RuntimeConfig::default(),
        )
    }

    pub fn with_output_permissions_logger_tls_roots_and_http_config<F>(
        output: F,
        permissions: RuntimePermissions,
        logger: RuntimeLogger,
        http_tls_roots: Option<Arc<rustls::RootCertStore>>,
        http_config: RuntimeHttpConfig,
        runtime_config: crate::runtime::config::RuntimeConfig,
    ) -> Self
    where
        F: FnMut(String) + 'static,
    {
        let env = Environment::new();
        let deadline = runtime_config.exec_timeout.map(|t| std::time::Instant::now() + t);
        let mut interpreter = Self {
            env,
            output: Box::new(output),
            current_loop: None,
            current_task: None,
            module_cache: HashMap::new(),
            loading_modules: Vec::new(),
            current_module_dir: None,
            permissions,
            logger,
            http_config,
            http_tls_roots,
            http_tls_config: RefCell::new(None),
            call_depth: 0,
            runtime_config,
            execution_deadline: deadline,
        };
        interpreter.install_natives();
        interpreter
    }

    pub fn permissions(&self) -> &RuntimePermissions {
        &self.permissions
    }

    pub fn logger(&self) -> &RuntimeLogger {
        &self.logger
    }

    pub fn http_config(&self) -> &RuntimeHttpConfig {
        &self.http_config
    }

    pub fn runtime_config(&self) -> &crate::runtime::config::RuntimeConfig {
        &self.runtime_config
    }

    fn check_timeout(&self, span: Span) -> IcooResult<()> {
        if let Some(deadline) = self.execution_deadline {
            if std::time::Instant::now() > deadline {
                return Err(IcooError::runtime("execution timed out", Some(span)));
            }
        }
        Ok(())
    }

    pub fn interpret(&mut self, program: &Program) -> IcooResult<()> {
        for stmt in &program.statements {
            self.execute(stmt)?;
        }
        Ok(())
    }

    fn install_natives(&mut self) {
        install_natives_into(&self.env);
    }

    pub(crate) fn emit_output(&mut self, value: String) {
        (self.output)(value);
    }
}

pub(super) fn install_natives_into(env: &EnvRef) {
    for (name, arity) in [
        ("print", 1),
        ("len", 1),
        ("str", 1),
        ("int", 1),
        ("float", 1),
        ("type", 1),
        ("EventLoop", 0),
        ("current_loop", 0),
        ("sleep", 1),
    ] {
        env.borrow_mut().define(
            name.to_string(),
            Value::NativeFunction(Rc::new(NativeFunction {
                name: name.to_string(),
                arity,
            })),
            true,
            BindingKind::Const,
        );
    }
    for name in ["math", "time", "json", "env", "Bytes", "Buffer"] {
        env.borrow_mut().define(
            name.to_string(),
            Value::NativeModule(Rc::new(NativeModule {
                name: name.to_string(),
            })),
            true,
            BindingKind::Const,
        );
    }
}
