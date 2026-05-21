use crate::parser::ast::{Expr, FieldKind, FunctionDecl, Stmt, TypeRef};
use crate::runtime::env::EnvRef;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Rc<RefCell<Vec<Value>>>),
    Map(Rc<RefCell<HashMap<String, Value>>>),
    Function(Rc<IcooFunction>),
    NativeFunction(Rc<NativeFunction>),
    NativeMethod(Rc<NativeMethod>),
    NativeModule(Rc<NativeModule>),
    NativeModuleMethod(Rc<NativeModuleMethod>),
    Module(Rc<IcooModule>),
    Coroutine(Rc<RefCell<IcooCoroutine>>),
    Task(Rc<RefCell<IcooTask>>),
    EventLoop(Rc<RefCell<IcooEventLoop>>),
    WebInoApp(Rc<RefCell<WebInoApp>>),
    WebInoResponse(Rc<RefCell<WebInoResponse>>),
    Class(Rc<IcooClass>),
    Instance(Rc<RefCell<Instance>>),
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display())
    }
}

impl Value {
    pub fn truthy(&self) -> bool {
        match self {
            Value::Nil => false,
            Value::Bool(value) => *value,
            _ => true,
        }
    }

    pub fn type_name(&self) -> String {
        match self {
            Value::Nil => "Nil".to_string(),
            Value::Bool(_) => "Bool".to_string(),
            Value::Int(_) => "Int".to_string(),
            Value::Float(_) => "Float".to_string(),
            Value::String(_) => "String".to_string(),
            Value::Array(_) => "Array".to_string(),
            Value::Map(_) => "Map".to_string(),
            Value::Function(_) => "Function".to_string(),
            Value::NativeFunction(_) | Value::NativeMethod(_) | Value::NativeModuleMethod(_) => {
                "Function".to_string()
            }
            Value::NativeModule(module) => module.name.clone(),
            Value::Module(_) => "Module".to_string(),
            Value::Coroutine(_) => "Coroutine".to_string(),
            Value::Task(_) => "Task".to_string(),
            Value::EventLoop(_) => "EventLoop".to_string(),
            Value::WebInoApp(_) => "WebInoApp".to_string(),
            Value::WebInoResponse(_) => "WebInoResponse".to_string(),
            Value::Class(class) => class.name.clone(),
            Value::Instance(instance) => instance.borrow().class.name.clone(),
        }
    }

    pub fn display(&self) -> String {
        match self {
            Value::Nil => "nil".to_string(),
            Value::Bool(value) => value.to_string(),
            Value::Int(value) => value.to_string(),
            Value::Float(value) => {
                let mut text = value.to_string();
                if !text.contains('.') {
                    text.push_str(".0");
                }
                text
            }
            Value::String(value) => value.clone(),
            Value::Array(values) => {
                let parts: Vec<String> = values.borrow().iter().map(Value::display).collect();
                format!("[{}]", parts.join(", "))
            }
            Value::Map(values) => {
                let mut parts: Vec<String> = values
                    .borrow()
                    .iter()
                    .map(|(key, value)| format!("{}: {}", key, value.display()))
                    .collect();
                parts.sort();
                format!("{{{}}}", parts.join(", "))
            }
            Value::Function(function) => format!("<fn {}>", function.decl.name.name),
            Value::NativeFunction(function) => format!("<native fn {}>", function.name),
            Value::NativeMethod(method) => format!("<native method {}>", method.name),
            Value::NativeModule(module) => format!("<module {}>", module.name),
            Value::NativeModuleMethod(method) => {
                format!("<native fn {}.{}>", method.module, method.name)
            }
            Value::Module(module) => format!("<module {}>", module.path.display()),
            Value::Coroutine(coroutine) => format!("<coroutine {}>", coroutine.borrow().name),
            Value::Task(task) => format!("<task {}>", task.borrow().id),
            Value::EventLoop(loop_ref) => {
                let loop_ref = loop_ref.borrow();
                format!("<event_loop {}>", loop_ref.backend.name())
            }
            Value::WebInoApp(_) => "<web_ino_app>".to_string(),
            Value::WebInoResponse(response) => {
                format!("<web_ino_response {}>", response.borrow().status)
            }
            Value::Class(class) => format!("<class {}>", class.name),
            Value::Instance(instance) => format!("<{} instance>", instance.borrow().class.name),
        }
    }
}

#[derive(Clone)]
pub struct IcooFunction {
    pub decl: FunctionDecl,
    pub closure: EnvRef,
    pub bound_self: Option<Value>,
    pub is_initializer: bool,
}

#[derive(Clone)]
pub struct IcooCoroutine {
    pub name: String,
    pub return_type: Option<TypeRef>,
    pub env: EnvRef,
    pub instructions: Vec<CoroutineInstr>,
    pub pc: usize,
    pub owner_task: Option<u64>,
}

impl fmt::Debug for IcooCoroutine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<coroutine {}>", self.name)
    }
}

#[derive(Debug, Clone)]
pub enum CoroutineInstr {
    Stmt(Stmt),
    JumpIfFalse { condition: Expr, target: usize },
    Jump { target: usize },
    Yield(Option<Expr>),
}

#[derive(Debug)]
pub struct IcooTask {
    pub id: u64,
    pub coroutine: Rc<RefCell<IcooCoroutine>>,
    pub state: TaskState,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub awaiters: Vec<Rc<RefCell<IcooTask>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Queued,
    Running,
    Waiting,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug)]
pub struct IcooEventLoop {
    pub next_task_id: u64,
    pub ready: VecDeque<Rc<RefCell<IcooTask>>>,
    pub timers: Vec<SleepTimer>,
    pub stopped: bool,
    pub backend: RuntimeBackendKind,
}

impl IcooEventLoop {
    pub fn new_local() -> Self {
        Self {
            next_task_id: 1,
            ready: VecDeque::new(),
            timers: Vec::new(),
            stopped: false,
            backend: RuntimeBackendKind::Local(LocalBackend),
        }
    }

    pub fn new_tokio(worker_threads: usize) -> Result<Self, String> {
        Ok(Self {
            next_task_id: 1,
            ready: VecDeque::new(),
            timers: Vec::new(),
            stopped: false,
            backend: RuntimeBackendKind::Tokio(TokioBackend::new(worker_threads)?),
        })
    }
}

#[derive(Debug)]
pub struct SleepTimer {
    pub due: Instant,
    pub task: Rc<RefCell<IcooTask>>,
}

#[derive(Clone)]
pub enum RuntimeBackendKind {
    Local(LocalBackend),
    Tokio(TokioBackend),
}

impl fmt::Debug for RuntimeBackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeBackendKind::Local(_) => write!(f, "LocalBackend"),
            RuntimeBackendKind::Tokio(backend) => f
                .debug_struct("TokioBackend")
                .field("worker_threads", &backend.worker_threads)
                .finish(),
        }
    }
}

impl RuntimeBackendKind {
    pub fn name(&self) -> &'static str {
        match self {
            RuntimeBackendKind::Local(backend) => backend.name(),
            RuntimeBackendKind::Tokio(backend) => backend.name(),
        }
    }

    pub fn worker_threads(&self) -> usize {
        match self {
            RuntimeBackendKind::Local(backend) => backend.worker_threads(),
            RuntimeBackendKind::Tokio(backend) => backend.worker_threads(),
        }
    }

    pub fn sleep_blocking(&self, duration: Duration) {
        match self {
            RuntimeBackendKind::Local(_) => std::thread::sleep(duration),
            RuntimeBackendKind::Tokio(backend) => backend.sleep_blocking(duration),
        }
    }
}

pub trait RuntimeBackend {
    fn name(&self) -> &'static str;
    fn worker_threads(&self) -> usize;
}

#[derive(Debug, Clone, Copy)]
pub struct LocalBackend;

impl RuntimeBackend for LocalBackend {
    fn name(&self) -> &'static str {
        "local"
    }

    fn worker_threads(&self) -> usize {
        1
    }
}

#[derive(Clone)]
pub struct TokioBackend {
    pub worker_threads: usize,
    pub runtime: Arc<tokio::runtime::Runtime>,
}

impl TokioBackend {
    pub fn new(worker_threads: usize) -> Result<Self, String> {
        let worker_threads = worker_threads.max(1);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(worker_threads)
            .enable_time()
            .build()
            .map_err(|err| format!("failed to create Tokio runtime: {}", err))?;
        Ok(Self {
            worker_threads,
            runtime: Arc::new(runtime),
        })
    }

    pub fn sleep_blocking(&self, duration: Duration) {
        self.runtime
            .block_on(async move { tokio::time::sleep(duration).await });
    }
}

impl RuntimeBackend for TokioBackend {
    fn name(&self) -> &'static str {
        "tokio"
    }

    fn worker_threads(&self) -> usize {
        self.worker_threads
    }
}

impl fmt::Debug for IcooFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<fn {}>", self.decl.name.name)
    }
}

impl IcooFunction {
    pub fn bind(&self, receiver: Value) -> Self {
        Self {
            decl: self.decl.clone(),
            closure: self.closure.clone(),
            bound_self: Some(receiver),
            is_initializer: self.is_initializer,
        }
    }
}

#[derive(Clone)]
pub struct NativeFunction {
    pub name: String,
    pub arity: usize,
}

impl fmt::Debug for NativeFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<native fn {}>", self.name)
    }
}

#[derive(Clone)]
pub struct NativeMethod {
    pub name: String,
    pub receiver: Value,
}

impl fmt::Debug for NativeMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<native method {}>", self.name)
    }
}

#[derive(Clone)]
pub struct NativeModule {
    pub name: String,
}

impl fmt::Debug for NativeModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<module {}>", self.name)
    }
}

#[derive(Clone)]
pub struct NativeModuleMethod {
    pub module: String,
    pub name: String,
}

impl fmt::Debug for NativeModuleMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<native fn {}.{}>", self.module, self.name)
    }
}

#[derive(Debug, Clone)]
pub struct IcooModule {
    pub path: PathBuf,
    pub exports: HashMap<String, Value>,
}

#[derive(Debug)]
pub struct WebInoApp {
    pub routes: HashMap<String, WebInoRoute>,
}

#[derive(Debug, Clone)]
pub struct WebInoRoute {
    pub method: String,
    pub path: String,
    pub handler: Value,
}

#[derive(Debug)]
pub struct WebInoResponse {
    pub status: i64,
    pub body: String,
    pub content_type: String,
    pub sent: bool,
}

#[derive(Clone)]
pub struct IcooClass {
    pub name: String,
    pub superclass: Option<Rc<IcooClass>>,
    pub fields: Vec<FieldDef>,
    pub methods: HashMap<String, Rc<IcooFunction>>,
}

impl fmt::Debug for IcooClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<class {}>", self.name)
    }
}

impl IcooClass {
    pub fn find_method(&self, name: &str) -> Option<Rc<IcooFunction>> {
        if let Some(method) = self.methods.get(name) {
            Some(method.clone())
        } else {
            self.superclass
                .as_ref()
                .and_then(|superclass| superclass.find_method(name))
        }
    }

    pub fn all_fields(&self) -> Vec<FieldDef> {
        let mut fields = self
            .superclass
            .as_ref()
            .map(|superclass| superclass.all_fields())
            .unwrap_or_default();
        fields.extend(self.fields.clone());
        fields
    }
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,
    pub kind: FieldKind,
    pub type_hint: TypeRef,
    pub initializer: Option<Expr>,
}

#[derive(Debug)]
pub struct Instance {
    pub class: Rc<IcooClass>,
    pub fields: HashMap<String, FieldValue>,
}

#[derive(Debug, Clone)]
pub struct FieldValue {
    pub value: Value,
    pub initialized: bool,
    pub kind: FieldKind,
    pub type_hint: TypeRef,
}
