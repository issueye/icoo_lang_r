use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use crate::parser::ast::*;
use crate::runtime::env::{BindingKind, EnvRef, Environment};
use crate::runtime::value::*;
use crate::{lexer, parser, resolver, typechecker};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct Interpreter {
    env: EnvRef,
    output: Box<dyn FnMut(String)>,
    current_loop: Option<Rc<RefCell<IcooEventLoop>>>,
    current_task: Option<Rc<RefCell<IcooTask>>>,
    module_cache: HashMap<PathBuf, Rc<IcooModule>>,
    loading_modules: Vec<PathBuf>,
    current_module_dir: Option<PathBuf>,
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
        let env = Environment::new();
        let mut interpreter = Self {
            env,
            output: Box::new(output),
            current_loop: None,
            current_task: None,
            module_cache: HashMap::new(),
            loading_modules: Vec::new(),
            current_module_dir: None,
        };
        interpreter.install_natives();
        interpreter
    }

    pub fn interpret(&mut self, program: &Program) -> IcooResult<()> {
        for stmt in &program.statements {
            self.execute(stmt)?;
        }
        Ok(())
    }

    pub fn interpret_file(&mut self, path: impl AsRef<Path>) -> IcooResult<()> {
        let path = canonical_module_path(path.as_ref()).map_err(|message| {
            IcooError::runtime(format!("module load error: {}", message), None)
        })?;
        self.load_module(&path)?;
        Ok(())
    }

    fn install_natives(&mut self) {
        install_natives_into(&self.env);
    }

    fn load_import_value(&mut self, source: &str, span: Span) -> IcooResult<Value> {
        if let Some(module_name) = importable_native_module_name(source) {
            return Ok(Value::NativeModule(Rc::new(NativeModule {
                name: module_name.to_string(),
            })));
        }
        self.load_relative_module(source, span).map(Value::Module)
    }

    fn load_relative_module(&mut self, source: &str, span: Span) -> IcooResult<Rc<IcooModule>> {
        if !source.ends_with(".icoo") {
            return Err(IcooError::runtime(
                "module path must end with '.icoo'",
                Some(span),
            ));
        }
        if !(source.starts_with("./") || source.starts_with("../")) {
            return Err(IcooError::runtime(
                "module path must start with './' or '../'",
                Some(span),
            ));
        }
        let base_dir = self
            .current_module_dir
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let path = canonical_module_path(&base_dir.join(source)).map_err(|message| {
            IcooError::runtime(format!("module load error: {}", message), Some(span))
        })?;
        self.load_module(&path)
    }

    fn load_module(&mut self, path: &Path) -> IcooResult<Rc<IcooModule>> {
        if let Some(module) = self.module_cache.get(path) {
            return Ok(module.clone());
        }
        if let Some(index) = self
            .loading_modules
            .iter()
            .position(|loading| loading == path)
        {
            let mut cycle = self.loading_modules[index..]
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>();
            cycle.push(path.display().to_string());
            return Err(IcooError::runtime(
                format!("module cycle detected: {}", cycle.join(" -> ")),
                None,
            ));
        }

        let source = std::fs::read_to_string(path).map_err(|err| {
            IcooError::runtime(
                format!("failed to read module '{}': {}", path.display(), err),
                None,
            )
        })?;
        let tokens = lexer::lex(&source)?;
        let program = parser::parse(tokens)?;
        resolver::resolve(&program)?;
        typechecker::check(&program)?;

        self.loading_modules.push(path.to_path_buf());
        let previous_env = self.env.clone();
        let previous_dir = self.current_module_dir.clone();
        let module_env = Environment::new();
        self.env = module_env.clone();
        install_natives_into(&self.env);
        self.current_module_dir = path.parent().map(Path::to_path_buf);

        let execution = (|| {
            for stmt in &program.statements {
                self.execute(stmt)?;
            }
            let exports = self.collect_exports(&program, &module_env)?;
            Ok(exports)
        })();

        self.env = previous_env;
        self.current_module_dir = previous_dir;
        self.loading_modules.pop();

        let exports = execution?;
        let module = Rc::new(IcooModule {
            path: path.to_path_buf(),
            exports,
        });
        self.module_cache.insert(path.to_path_buf(), module.clone());
        Ok(module)
    }

    fn collect_exports(
        &self,
        program: &Program,
        module_env: &EnvRef,
    ) -> IcooResult<HashMap<String, Value>> {
        let mut exports = HashMap::new();
        for stmt in &program.statements {
            if let Stmt::ExportDecl(inner) = stmt {
                let (name, span) = export_name(inner).ok_or_else(|| {
                    IcooError::runtime("exported statement has no binding name", None)
                })?;
                if exports.contains_key(&name) {
                    return Err(IcooError::runtime(
                        format!("duplicate export '{}'", name),
                        Some(span),
                    ));
                }
                let value = module_env.borrow().get(&name, span)?;
                exports.insert(name, value);
            }
        }
        Ok(exports)
    }
}

fn install_natives_into(env: &EnvRef) {
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
    for name in ["math", "time", "json", "env"] {
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

impl Interpreter {
    fn execute(&mut self, stmt: &Stmt) -> IcooResult<()> {
        match stmt {
            Stmt::ImportModule {
                source,
                alias,
                span,
            } => {
                let module = self.load_import_value(source, *span)?;
                self.env
                    .borrow_mut()
                    .define(alias.name.clone(), module, true, BindingKind::Const);
                Ok(())
            }
            Stmt::ImportNames {
                source,
                items,
                span,
            } => {
                let module = self.load_import_value(source, *span)?;
                for item in items {
                    let local = item.alias.as_ref().unwrap_or(&item.name);
                    let value = imported_member(&module, &item.name.name, item.name.span)?;
                    self.env.borrow_mut().define(
                        local.name.clone(),
                        value,
                        true,
                        BindingKind::Const,
                    );
                }
                Ok(())
            }
            Stmt::ExportDecl(inner) => self.execute(inner),
            Stmt::Let(decl) => self.define_binding(decl, BindingKind::Mutable),
            Stmt::Const(decl) => self.define_binding(decl, BindingKind::Const),
            Stmt::Final(decl) => self.define_binding(decl, BindingKind::Final),
            Stmt::Function(decl) => {
                let function = IcooFunction {
                    decl: decl.clone(),
                    closure: self.env.clone(),
                    bound_self: None,
                    is_initializer: false,
                };
                self.env.borrow_mut().define(
                    decl.name.name.clone(),
                    Value::Function(Rc::new(function)),
                    true,
                    BindingKind::Const,
                );
                Ok(())
            }
            Stmt::Class(decl) => self.execute_class(decl),
            Stmt::If {
                condition,
                then_branch,
                elifs,
                else_branch,
            } => {
                if self.eval(condition)?.truthy() {
                    self.execute_block(then_branch, Environment::child(self.env.clone()))
                } else {
                    for (condition, body) in elifs {
                        if self.eval(condition)?.truthy() {
                            return self.execute_block(body, Environment::child(self.env.clone()));
                        }
                    }
                    if let Some(body) = else_branch {
                        self.execute_block(body, Environment::child(self.env.clone()))
                    } else {
                        Ok(())
                    }
                }
            }
            Stmt::While { condition, body } => {
                while self.eval(condition)?.truthy() {
                    match self.execute_block(body, Environment::child(self.env.clone())) {
                        Err(IcooError::Break) => break,
                        Err(IcooError::Continue) => continue,
                        other => other?,
                    }
                }
                Ok(())
            }
            Stmt::Return { value, .. } => {
                let value = if let Some(value) = value {
                    self.eval(value)?
                } else {
                    Value::Nil
                };
                Err(IcooError::Return(value))
            }
            Stmt::Yield { span, .. } => Err(IcooError::runtime(
                "yield can only be used inside an async fn",
                Some(*span),
            )),
            Stmt::Break(_) => Err(IcooError::Break),
            Stmt::Continue(_) => Err(IcooError::Continue),
            Stmt::Expr(expr) => {
                self.eval(expr)?;
                Ok(())
            }
        }
    }

    fn define_binding(&mut self, decl: &BindingDecl, kind: BindingKind) -> IcooResult<()> {
        let (value, initialized) = if let Some(initializer) = &decl.initializer {
            let value = self.eval(initializer)?;
            if let Some(type_hint) = &decl.type_hint {
                check_value_type(
                    &value,
                    type_hint,
                    &format!("binding '{}'", decl.name.name),
                    decl.name.span,
                )?;
            }
            (value, true)
        } else {
            (Value::Nil, false)
        };
        self.env
            .borrow_mut()
            .define(decl.name.name.clone(), value, initialized, kind);
        Ok(())
    }

    fn execute_class(&mut self, decl: &ClassDecl) -> IcooResult<()> {
        let superclass = if let Some(super_name) = &decl.superclass {
            match self.env.borrow().get(&super_name.name, super_name.span)? {
                Value::Class(class) => Some(class),
                _ => {
                    return Err(IcooError::runtime(
                        format!("superclass '{}' is not a class", super_name.name),
                        Some(super_name.span),
                    ))
                }
            }
        } else {
            None
        };

        let mut seen = HashSet::new();
        if let Some(superclass) = &superclass {
            for field in superclass.all_fields() {
                seen.insert(field.name);
            }
        }
        let mut fields = Vec::new();
        for field in &decl.fields {
            if !seen.insert(field.name.name.clone()) {
                return Err(IcooError::runtime(
                    format!(
                        "field '{}' is already declared in this inheritance chain",
                        field.name.name
                    ),
                    Some(field.name.span),
                ));
            }
            fields.push(FieldDef {
                name: field.name.name.clone(),
                kind: field.kind,
                type_hint: field.type_hint.clone(),
                initializer: field.initializer.clone(),
            });
        }

        let method_closure = if let Some(superclass) = &superclass {
            let env = Environment::child(self.env.clone());
            env.borrow_mut().define(
                "super".to_string(),
                Value::Class(superclass.clone()),
                true,
                BindingKind::Const,
            );
            env
        } else {
            self.env.clone()
        };

        let mut methods = HashMap::new();
        for method in &decl.methods {
            methods.insert(
                method.name.name.clone(),
                Rc::new(IcooFunction {
                    decl: method.clone(),
                    closure: method_closure.clone(),
                    bound_self: None,
                    is_initializer: method.name.name == "init",
                }),
            );
        }

        let class = Rc::new(IcooClass {
            name: decl.name.name.clone(),
            superclass,
            fields,
            methods,
        });
        self.env.borrow_mut().define(
            decl.name.name.clone(),
            Value::Class(class),
            true,
            BindingKind::Const,
        );
        Ok(())
    }

    fn execute_block(&mut self, statements: &[Stmt], env: EnvRef) -> IcooResult<()> {
        let previous = self.env.clone();
        self.env = env;
        let mut result = Ok(());
        for stmt in statements {
            if let Err(err) = self.execute(stmt) {
                result = Err(err);
                break;
            }
        }
        self.env = previous;
        result
    }

    fn eval(&mut self, expr: &Expr) -> IcooResult<Value> {
        match expr {
            Expr::Literal(literal, _) => Ok(match literal {
                Literal::Nil => Value::Nil,
                Literal::Bool(value) => Value::Bool(*value),
                Literal::Int(value) => Value::Int(*value),
                Literal::Float(value) => Value::Float(*value),
                Literal::String(value) => Value::String(value.clone()),
            }),
            Expr::Variable(name) => self.env.borrow().get(&name.name, name.span),
            Expr::Self_(span) => self.env.borrow().get("self", *span),
            Expr::Super(span) => self.env.borrow().get("super", *span),
            Expr::Array(values, _) => {
                let mut result = Vec::new();
                for value in values {
                    result.push(self.eval(value)?);
                }
                Ok(Value::Array(Rc::new(RefCell::new(result))))
            }
            Expr::Map(entries, _) => {
                let mut result = HashMap::new();
                for (key, value) in entries {
                    result.insert(key.clone(), self.eval(value)?);
                }
                Ok(Value::Map(Rc::new(RefCell::new(result))))
            }
            Expr::Template(parts, _) => {
                let mut result = String::new();
                for part in parts {
                    match part {
                        TemplatePart::Text(text) => result.push_str(text),
                        TemplatePart::Expr(expr) => result.push_str(&self.eval(expr)?.display()),
                    }
                }
                Ok(Value::String(result))
            }
            Expr::Unary { op, right, span } => {
                let right = self.eval(right)?;
                match op {
                    UnaryOp::Not => Ok(Value::Bool(!right.truthy())),
                    UnaryOp::Negate => match right {
                        Value::Int(value) => Ok(Value::Int(-value)),
                        Value::Float(value) => Ok(Value::Float(-value)),
                        _ => Err(IcooError::runtime("operand must be a number", Some(*span))),
                    },
                }
            }
            Expr::Binary {
                left,
                op,
                right,
                span,
            } => {
                let left = self.eval(left)?;
                let right = self.eval(right)?;
                self.eval_binary(left, *op, right, *span)
            }
            Expr::Logical {
                left, op, right, ..
            } => {
                let left = self.eval(left)?;
                match op {
                    LogicalOp::Or if left.truthy() => Ok(left),
                    LogicalOp::And if !left.truthy() => Ok(left),
                    _ => self.eval(right),
                }
            }
            Expr::Assign {
                target,
                value,
                span,
            } => {
                let value = self.eval(value)?;
                match target.as_ref() {
                    Expr::Variable(name) => {
                        self.env
                            .borrow_mut()
                            .assign(&name.name, value.clone(), *span)?;
                        Ok(value)
                    }
                    Expr::Get { object, name, .. } => {
                        let object = self.eval(object)?;
                        self.set_property(object, &name.name, value.clone(), *span)?;
                        Ok(value)
                    }
                    _ => Err(IcooError::runtime("invalid assignment target", Some(*span))),
                }
            }
            Expr::Get { object, name, span } => {
                if matches!(object.as_ref(), Expr::Super(_)) {
                    return self.eval_super_get(&name.name, *span);
                }
                let object = self.eval(object)?;
                self.get_property(object, &name.name, *span)
            }
            Expr::Call { callee, args, span } => {
                let callee = self.eval(callee)?;
                let mut values = Vec::new();
                for arg in args {
                    values.push(self.eval(arg)?);
                }
                self.call_value(callee, values, *span)
            }
            Expr::Await { task, span } => {
                if self.current_task.is_none() {
                    return Err(IcooError::runtime(
                        "await can only be used inside an async fn",
                        Some(*span),
                    ));
                }
                let value = self.eval(task)?;
                let Value::Task(task) = value else {
                    return Err(IcooError::runtime("await expects a Task", Some(*span)));
                };
                let state = task.borrow().state;
                match state {
                    TaskState::Done => {
                        let result = task.borrow().result.clone().unwrap_or(Value::Nil);
                        Ok(result)
                    }
                    TaskState::Failed => {
                        let error = task
                            .borrow()
                            .error
                            .clone()
                            .unwrap_or_else(|| "task failed".to_string());
                        Err(IcooError::runtime(error, Some(*span)))
                    }
                    TaskState::Cancelled => {
                        Err(IcooError::runtime("task was cancelled", Some(*span)))
                    }
                    _ => Err(IcooError::Await(Value::Task(task))),
                }
            }
        }
    }

    fn eval_binary(
        &self,
        left: Value,
        op: BinaryOp,
        right: Value,
        span: Span,
    ) -> IcooResult<Value> {
        match op {
            BinaryOp::Add => match (left, right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 + b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + b as f64)),
                (Value::String(a), Value::String(b)) => Ok(Value::String(a + &b)),
                (Value::String(a), b) => Ok(Value::String(a + &b.display())),
                (a, Value::String(b)) => Ok(Value::String(a.display() + &b)),
                _ => Err(IcooError::runtime(
                    "operands must be numbers or strings",
                    Some(span),
                )),
            },
            BinaryOp::Subtract => numeric(left, right, span, |a, b| a - b, |a, b| a - b),
            BinaryOp::Multiply => numeric(left, right, span, |a, b| a * b, |a, b| a * b),
            BinaryOp::Divide => numeric_float(left, right, span, |a, b| a / b),
            BinaryOp::Remainder => numeric(left, right, span, |a, b| a % b, |a, b| a % b),
            BinaryOp::Equal => Ok(Value::Bool(value_equal(&left, &right))),
            BinaryOp::NotEqual => Ok(Value::Bool(!value_equal(&left, &right))),
            BinaryOp::Less => compare(left, right, span, |a, b| a < b),
            BinaryOp::LessEqual => compare(left, right, span, |a, b| a <= b),
            BinaryOp::Greater => compare(left, right, span, |a, b| a > b),
            BinaryOp::GreaterEqual => compare(left, right, span, |a, b| a >= b),
        }
    }

    fn get_property(&mut self, object: Value, name: &str, span: Span) -> IcooResult<Value> {
        if let Value::Module(module) = &object {
            return module.exports.get(name).cloned().ok_or_else(|| {
                IcooError::runtime(
                    format!(
                        "module '{}' has no export '{}'",
                        module.path.display(),
                        name
                    ),
                    Some(span),
                )
            });
        }
        if let Value::NativeModule(module) = &object {
            if has_native_module_method(&module.name, name) {
                return Ok(Value::NativeModuleMethod(Rc::new(NativeModuleMethod {
                    module: module.name.clone(),
                    name: name.to_string(),
                })));
            }
        }
        if let Value::Instance(instance) = &object {
            let field = instance.borrow().fields.get(name).cloned();
            if let Some(field) = field {
                if !field.initialized {
                    return Err(IcooError::runtime(
                        format!("field '{}' is not initialized", name),
                        Some(span),
                    ));
                }
                return Ok(field.value);
            }
            if let Some(method) = instance.borrow().class.find_method(name) {
                let bound = method.bind(Value::Instance(instance.clone()));
                return Ok(Value::Function(Rc::new(bound)));
            }
        }
        if self.has_native_method(&object, name) {
            return Ok(Value::NativeMethod(Rc::new(NativeMethod {
                name: name.to_string(),
                receiver: object,
            })));
        }
        Err(IcooError::runtime(
            format!(
                "type '{}' has no property or method '{}'",
                object.type_name(),
                name
            ),
            Some(span),
        ))
    }

    fn set_property(
        &mut self,
        object: Value,
        name: &str,
        value: Value,
        span: Span,
    ) -> IcooResult<()> {
        let Value::Instance(instance) = object else {
            return Err(IcooError::runtime("only instances have fields", Some(span)));
        };
        let mut instance_ref = instance.borrow_mut();
        let Some(field) = instance_ref.fields.get_mut(name) else {
            return Err(IcooError::runtime(
                format!(
                    "cannot assign undeclared field '{}' on class '{}'",
                    name, instance_ref.class.name
                ),
                Some(span),
            ));
        };
        match field.kind {
            FieldKind::Mutable => {
                check_value_type(&value, &field.type_hint, &format!("field '{}'", name), span)?;
                field.value = value;
                field.initialized = true;
                Ok(())
            }
            FieldKind::Const => Err(IcooError::runtime(
                format!("cannot assign const field '{}'", name),
                Some(span),
            )),
            FieldKind::Final if !field.initialized => {
                check_value_type(&value, &field.type_hint, &format!("field '{}'", name), span)?;
                field.value = value;
                field.initialized = true;
                Ok(())
            }
            FieldKind::Final => Err(IcooError::runtime(
                format!("final field '{}' can only be assigned once", name),
                Some(span),
            )),
        }
    }

    fn eval_super_get(&mut self, name: &str, span: Span) -> IcooResult<Value> {
        let superclass = match self.env.borrow().get("super", span)? {
            Value::Class(class) => class,
            _ => return Err(IcooError::runtime("'super' is not a class", Some(span))),
        };
        let receiver = self.env.borrow().get("self", span)?;
        let Some(method) = superclass.find_method(name) else {
            return Err(IcooError::runtime(
                format!("undefined superclass method '{}'", name),
                Some(span),
            ));
        };
        Ok(Value::Function(Rc::new(method.bind(receiver))))
    }

    fn call_value(&mut self, callee: Value, args: Vec<Value>, span: Span) -> IcooResult<Value> {
        match callee {
            Value::Function(function) => self.call_function(function, args, span),
            Value::NativeFunction(function) => self.call_native_function(&function, args, span),
            Value::NativeMethod(method) => self.call_native_method(&method, args, span),
            Value::NativeModuleMethod(method) => {
                self.call_native_module_method(&method, args, span)
            }
            Value::Class(class) => self.call_class(class, args, span),
            _ => Err(IcooError::runtime(
                format!("type '{}' is not callable", callee.type_name()),
                Some(span),
            )),
        }
    }

    fn call_function(
        &mut self,
        function: Rc<IcooFunction>,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        let bound_offset = usize::from(function.bound_self.is_some());
        let expected = function.decl.params.len().saturating_sub(bound_offset);
        if args.len() != expected {
            return Err(IcooError::runtime(
                format!("expected {} arguments but got {}", expected, args.len()),
                Some(span),
            ));
        }

        let env = Environment::child(function.closure.clone());
        let mut arg_index = 0;
        for (param_index, param) in function.decl.params.iter().enumerate() {
            if param_index == 0 {
                if let Some(receiver) = &function.bound_self {
                    if let Some(type_hint) = &param.type_hint {
                        check_value_type(
                            receiver,
                            type_hint,
                            &format!("parameter '{}'", param.name.name),
                            span,
                        )?;
                    }
                    env.borrow_mut().define(
                        param.name.name.clone(),
                        receiver.clone(),
                        true,
                        BindingKind::Const,
                    );
                    continue;
                }
            }
            let arg = args[arg_index].clone();
            if let Some(type_hint) = &param.type_hint {
                check_value_type(
                    &arg,
                    type_hint,
                    &format!("parameter '{}'", param.name.name),
                    span,
                )?;
            }
            env.borrow_mut()
                .define(param.name.name.clone(), arg, true, BindingKind::Mutable);
            arg_index += 1;
        }

        if function.decl.is_coroutine {
            return Ok(Value::Coroutine(Rc::new(RefCell::new(IcooCoroutine {
                name: function.decl.name.name.clone(),
                return_type: function.decl.return_type.clone(),
                env,
                instructions: compile_coroutine_body(&function.decl.body),
                pc: 0,
                owner_task: None,
            }))));
        }

        let result = self.execute_block(&function.decl.body, env);
        match result {
            Err(IcooError::Return(value)) => {
                if function.is_initializer {
                    Ok(function.bound_self.clone().unwrap_or(Value::Nil))
                } else {
                    self.check_function_return(&function, &value, span)?;
                    Ok(value)
                }
            }
            Err(err) => Err(err),
            Ok(()) if function.is_initializer => {
                Ok(function.bound_self.clone().unwrap_or(Value::Nil))
            }
            Ok(()) => {
                self.check_function_return(&function, &Value::Nil, span)?;
                Ok(Value::Nil)
            }
        }
    }

    fn check_function_return(
        &self,
        function: &IcooFunction,
        value: &Value,
        span: Span,
    ) -> IcooResult<()> {
        if let Some(return_type) = &function.decl.return_type {
            check_value_type(
                value,
                return_type,
                &format!("return value of '{}'", function.decl.name.name),
                span,
            )?;
        }
        Ok(())
    }

    fn call_class(
        &mut self,
        class: Rc<IcooClass>,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        let fields = class.all_fields();
        let mut instance_fields = HashMap::new();
        for field in &fields {
            instance_fields.insert(
                field.name.clone(),
                FieldValue {
                    value: Value::Nil,
                    initialized: false,
                    kind: field.kind,
                    type_hint: field.type_hint.clone(),
                },
            );
        }
        let instance = Rc::new(RefCell::new(Instance {
            class: class.clone(),
            fields: instance_fields,
        }));
        let receiver = Value::Instance(instance.clone());

        for field in &fields {
            if let Some(initializer) = &field.initializer {
                let value = self.eval(initializer)?;
                check_value_type(
                    &value,
                    &field.type_hint,
                    &format!("field '{}'", field.name),
                    span,
                )?;
                let mut instance_ref = instance.borrow_mut();
                let Some(slot) = instance_ref.fields.get_mut(&field.name) else {
                    return Err(IcooError::runtime(
                        format!("internal error: missing field '{}'", field.name),
                        Some(span),
                    ));
                };
                slot.value = value;
                slot.initialized = true;
            }
        }

        if let Some(init) = class.find_method("init") {
            let bound = init.bind(receiver.clone());
            self.call_function(Rc::new(bound), args, span)?;
        } else if !args.is_empty() {
            return Err(IcooError::runtime(
                format!(
                    "class '{}' expected 0 arguments but got {}",
                    class.name,
                    args.len()
                ),
                Some(span),
            ));
        }

        let missing: Vec<String> = instance
            .borrow()
            .fields
            .iter()
            .filter(|(_, field)| !field.initialized)
            .map(|(name, _)| name.clone())
            .collect();
        if !missing.is_empty() {
            return Err(IcooError::runtime(
                format!(
                    "class '{}' did not initialize required fields: {}",
                    class.name,
                    missing.join(", ")
                ),
                Some(span),
            ));
        }
        Ok(receiver)
    }

    fn call_native_function(
        &mut self,
        function: &NativeFunction,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        let valid_arity = if function.name == "EventLoop" {
            args.len() <= 1
        } else {
            args.len() == function.arity
        };
        if !valid_arity {
            return Err(IcooError::runtime(
                format!(
                    "expected {} arguments but got {}",
                    function.arity,
                    args.len()
                ),
                Some(span),
            ));
        }
        match function.name.as_str() {
            "print" => {
                (self.output)(args[0].display());
                Ok(Value::Nil)
            }
            "len" => self.call_native_method(
                &NativeMethod {
                    name: "len".to_string(),
                    receiver: args[0].clone(),
                },
                Vec::new(),
                span,
            ),
            "str" => Ok(Value::String(args[0].display())),
            "type" => Ok(Value::String(args[0].type_name())),
            "EventLoop" => {
                let workers = if args.is_empty() {
                    std::thread::available_parallelism()
                        .map(|count| count.get())
                        .unwrap_or(1)
                } else {
                    let workers = expect_int(&args[0], span)?;
                    if workers <= 0 {
                        return Err(IcooError::runtime(
                            "EventLoop() worker count must be positive",
                            Some(span),
                        ));
                    }
                    workers as usize
                };
                let event_loop = IcooEventLoop::new_tokio(workers)
                    .map_err(|message| IcooError::runtime(message, Some(span)))?;
                Ok(Value::EventLoop(Rc::new(RefCell::new(event_loop))))
            }
            "current_loop" => self
                .current_loop
                .as_ref()
                .cloned()
                .map(Value::EventLoop)
                .ok_or_else(|| {
                    IcooError::runtime(
                        "current_loop() can only be used inside a running task",
                        Some(span),
                    )
                }),
            "sleep" => {
                let millis = expect_int(&args[0], span)?;
                if millis < 0 {
                    return Err(IcooError::runtime(
                        "sleep() expects non-negative milliseconds",
                        Some(span),
                    ));
                }
                let Some(loop_ref) = self.current_loop.clone() else {
                    return Err(IcooError::runtime(
                        "sleep() can only be used inside a running task",
                        Some(span),
                    ));
                };
                Ok(Value::Task(schedule_sleep_task(loop_ref, millis as u64)))
            }
            "int" => match &args[0] {
                Value::Int(value) => Ok(Value::Int(*value)),
                Value::Float(value) => Ok(Value::Int(*value as i64)),
                Value::Bool(value) => Ok(Value::Int(i64::from(*value))),
                Value::String(value) => value.parse::<i64>().map(Value::Int).map_err(|_| {
                    IcooError::runtime(format!("cannot convert '{}' to Int", value), Some(span))
                }),
                _ => Err(IcooError::runtime(
                    "value cannot be converted to Int",
                    Some(span),
                )),
            },
            "float" => match &args[0] {
                Value::Float(value) => Ok(Value::Float(*value)),
                Value::Int(value) => Ok(Value::Float(*value as f64)),
                Value::String(value) => value.parse::<f64>().map(Value::Float).map_err(|_| {
                    IcooError::runtime(format!("cannot convert '{}' to Float", value), Some(span))
                }),
                _ => Err(IcooError::runtime(
                    "value cannot be converted to Float",
                    Some(span),
                )),
            },
            _ => Err(IcooError::runtime("unknown native function", Some(span))),
        }
    }

    fn has_native_method(&self, receiver: &Value, name: &str) -> bool {
        if matches!(name, "to_string" | "type_name") {
            return true;
        }
        match receiver {
            Value::String(_) => matches!(name, "len" | "is_empty" | "contains"),
            Value::Array(_) => matches!(
                name,
                "len"
                    | "is_empty"
                    | "push"
                    | "pop"
                    | "shift"
                    | "unshift"
                    | "at"
                    | "includes"
                    | "index_of"
                    | "slice"
                    | "splice"
                    | "join"
                    | "reverse"
                    | "for_each"
                    | "map"
                    | "filter"
                    | "reduce"
                    | "find"
                    | "find_index"
                    | "some"
                    | "every"
            ),
            Value::Map(_) => matches!(
                name,
                "len"
                    | "is_empty"
                    | "size"
                    | "has"
                    | "get"
                    | "set"
                    | "delete"
                    | "clear"
                    | "keys"
                    | "values"
                    | "entries"
                    | "for_each"
            ),
            Value::EventLoop(_) => matches!(
                name,
                "spawn"
                    | "run"
                    | "run_until"
                    | "stop"
                    | "is_stopped"
                    | "backend_name"
                    | "worker_threads"
            ),
            Value::WebInoApp(_) => {
                matches!(
                    name,
                    "get" | "post" | "listen_once" | "listen" | "listen_with_workers"
                )
            }
            Value::WebInoResponse(_) => {
                matches!(name, "status" | "send" | "json" | "write" | "end")
            }
            Value::Task(_) => matches!(name, "is_done" | "is_failed" | "result" | "cancel"),
            Value::Bool(_) => matches!(name, "to_int"),
            Value::Int(_) => matches!(name, "to_float" | "abs"),
            Value::Float(_) => matches!(name, "to_int" | "abs"),
            Value::Instance(_)
            | Value::Nil
            | Value::Function(_)
            | Value::Coroutine(_)
            | Value::Class(_)
            | Value::Module(_)
            | Value::NativeModule(_) => true,
            Value::NativeFunction(_) | Value::NativeMethod(_) | Value::NativeModuleMethod(_) => {
                true
            }
        }
    }

    fn call_native_module_method(
        &mut self,
        method: &NativeModuleMethod,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match (native_module_kind(&method.module), method.name.as_str()) {
            ("math", "abs") => {
                expect_arity(&args, 1, span)?;
                match &args[0] {
                    Value::Int(value) => Ok(Value::Int(value.abs())),
                    Value::Float(value) => Ok(Value::Float(value.abs())),
                    _ => Err(IcooError::runtime("expected numeric argument", Some(span))),
                }
            }
            ("math", "floor") => {
                expect_arity(&args, 1, span)?;
                Ok(Value::Int(expect_number(&args[0], span)?.floor() as i64))
            }
            ("math", "ceil") => {
                expect_arity(&args, 1, span)?;
                Ok(Value::Int(expect_number(&args[0], span)?.ceil() as i64))
            }
            ("math", "round") => {
                expect_arity(&args, 1, span)?;
                Ok(Value::Int(expect_number(&args[0], span)?.round() as i64))
            }
            ("math", "min") => {
                expect_arity(&args, 2, span)?;
                numeric_min_max(&args[0], &args[1], span, f64::min)
            }
            ("math", "max") => {
                expect_arity(&args, 2, span)?;
                numeric_min_max(&args[0], &args[1], span, f64::max)
            }
            ("math", "random") => {
                expect_arity(&args, 0, span)?;
                let nanos = now_duration(span)?.subsec_nanos();
                Ok(Value::Float((nanos as f64) / 1_000_000_000.0))
            }
            ("time", "now_ms") => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(now_duration(span)?.as_millis() as i64))
            }
            ("time", "now_sec") => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(now_duration(span)?.as_secs() as i64))
            }
            ("json", "stringify") => {
                expect_arity(&args, 1, span)?;
                serde_json::to_string(&value_to_json(&args[0], span)?)
                    .map(Value::String)
                    .map_err(|err| {
                        IcooError::runtime(format!("json.stringify() failed: {}", err), Some(span))
                    })
            }
            ("json", "parse") => {
                expect_arity(&args, 1, span)?;
                let text = expect_string(&args[0], span)?;
                let parsed = serde_json::from_str::<serde_json::Value>(&text).map_err(|err| {
                    IcooError::runtime(format!("json.parse() failed: {}", err), Some(span))
                })?;
                json_to_value(parsed, span)
            }
            ("yaml", "stringify") => {
                expect_arity(&args, 1, span)?;
                serde_yml::to_string(&value_to_json(&args[0], span)?)
                    .map(Value::String)
                    .map_err(|err| {
                        IcooError::runtime(format!("yaml.stringify() failed: {}", err), Some(span))
                    })
            }
            ("yaml", "parse") => {
                expect_arity(&args, 1, span)?;
                let text = expect_string(&args[0], span)?;
                let parsed = serde_yml::from_str::<serde_json::Value>(&text).map_err(|err| {
                    IcooError::runtime(format!("yaml.parse() failed: {}", err), Some(span))
                })?;
                json_to_value(parsed, span)
            }
            ("toml", "stringify") => {
                expect_arity(&args, 1, span)?;
                toml::to_string(&value_to_toml(&args[0], span)?)
                    .map(Value::String)
                    .map_err(|err| {
                        IcooError::runtime(format!("toml.stringify() failed: {}", err), Some(span))
                    })
            }
            ("toml", "parse") => {
                expect_arity(&args, 1, span)?;
                let text = expect_string(&args[0], span)?;
                let parsed = toml::from_str::<toml::Value>(&text).map_err(|err| {
                    IcooError::runtime(format!("toml.parse() failed: {}", err), Some(span))
                })?;
                toml_to_value(parsed, span)
            }
            ("env", "cwd") => {
                expect_arity(&args, 0, span)?;
                std::env::current_dir()
                    .map(|path| Value::String(path.to_string_lossy().into_owned()))
                    .map_err(|err| {
                        IcooError::runtime(format!("env.cwd() failed: {}", err), Some(span))
                    })
            }
            ("env", "args") => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Array(Rc::new(RefCell::new(
                    std::env::args().map(Value::String).collect(),
                ))))
            }
            ("env", "get") => {
                expect_arity(&args, 1, span)?;
                let name = expect_string(&args[0], span)?;
                Ok(std::env::var(name).map(Value::String).unwrap_or(Value::Nil))
            }
            ("env", "has") => {
                expect_arity(&args, 1, span)?;
                let name = expect_string(&args[0], span)?;
                Ok(Value::Bool(std::env::var_os(name).is_some()))
            }
            ("io", "print") => {
                expect_arity(&args, 1, span)?;
                (self.output)(args[0].display());
                Ok(Value::Nil)
            }
            ("io.fs", "exists") => {
                expect_arity(&args, 1, span)?;
                let path = expect_string(&args[0], span)?;
                Ok(Value::Bool(std::path::Path::new(&path).exists()))
            }
            ("io.fs", "is_file") => {
                expect_arity(&args, 1, span)?;
                let path = expect_string(&args[0], span)?;
                Ok(Value::Bool(std::path::Path::new(&path).is_file()))
            }
            ("io.fs", "is_dir") => {
                expect_arity(&args, 1, span)?;
                let path = expect_string(&args[0], span)?;
                Ok(Value::Bool(std::path::Path::new(&path).is_dir()))
            }
            ("io.fs", "read_text") => {
                expect_arity(&args, 1, span)?;
                let path = expect_string(&args[0], span)?;
                std::fs::read_to_string(&path)
                    .map(Value::String)
                    .map_err(|err| {
                        IcooError::runtime(format!("io.fs.read_text() failed: {}", err), Some(span))
                    })
            }
            ("io.fs", "write_text") => {
                expect_arity(&args, 2, span)?;
                let path = expect_string(&args[0], span)?;
                let content = expect_string(&args[1], span)?;
                std::fs::write(&path, content)
                    .map(|_| Value::Nil)
                    .map_err(|err| {
                        IcooError::runtime(
                            format!("io.fs.write_text() failed: {}", err),
                            Some(span),
                        )
                    })
            }
            ("io.fs", "append_text") => {
                expect_arity(&args, 2, span)?;
                let path = expect_string(&args[0], span)?;
                let content = expect_string(&args[1], span)?;
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .and_then(|mut file| {
                        use std::io::Write;
                        file.write_all(content.as_bytes())
                    })
                    .map(|_| Value::Nil)
                    .map_err(|err| {
                        IcooError::runtime(
                            format!("io.fs.append_text() failed: {}", err),
                            Some(span),
                        )
                    })
            }
            ("io.fs", "list_dir") => {
                expect_arity(&args, 1, span)?;
                let path = expect_string(&args[0], span)?;
                let mut entries = Vec::new();
                for entry in std::fs::read_dir(&path).map_err(|err| {
                    IcooError::runtime(format!("io.fs.list_dir() failed: {}", err), Some(span))
                })? {
                    let entry = entry.map_err(|err| {
                        IcooError::runtime(format!("io.fs.list_dir() failed: {}", err), Some(span))
                    })?;
                    entries.push(Value::String(
                        entry.file_name().to_string_lossy().into_owned(),
                    ));
                }
                entries.sort_by_key(Value::display);
                Ok(Value::Array(Rc::new(RefCell::new(entries))))
            }
            ("os", "name") => {
                expect_arity(&args, 0, span)?;
                Ok(Value::String(std::env::consts::OS.to_string()))
            }
            ("os", "family") => {
                expect_arity(&args, 0, span)?;
                Ok(Value::String(std::env::consts::FAMILY.to_string()))
            }
            ("os", "arch") => {
                expect_arity(&args, 0, span)?;
                Ok(Value::String(std::env::consts::ARCH.to_string()))
            }
            ("os", "pid") => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(std::process::id() as i64))
            }
            ("os", "cwd") => {
                expect_arity(&args, 0, span)?;
                std::env::current_dir()
                    .map(|path| Value::String(path.to_string_lossy().into_owned()))
                    .map_err(|err| {
                        IcooError::runtime(format!("os.cwd() failed: {}", err), Some(span))
                    })
            }
            ("os", "args") => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Array(Rc::new(RefCell::new(
                    std::env::args().map(Value::String).collect(),
                ))))
            }
            ("os", "exe_path") => {
                expect_arity(&args, 0, span)?;
                Ok(std::env::current_exe()
                    .map(|path| Value::String(path.to_string_lossy().into_owned()))
                    .unwrap_or(Value::Nil))
            }
            ("os", "get_env") => {
                expect_arity(&args, 1, span)?;
                let name = expect_string(&args[0], span)?;
                Ok(std::env::var(name).map(Value::String).unwrap_or(Value::Nil))
            }
            ("os", "has_env") => {
                expect_arity(&args, 1, span)?;
                let name = expect_string(&args[0], span)?;
                Ok(Value::Bool(std::env::var_os(name).is_some()))
            }
            ("net.http.client", "get") => {
                expect_arity(&args, 1, span)?;
                let url = expect_string(&args[0], span)?;
                http_client_request("GET", &url, "", span)
            }
            ("net.http.client", "post") => {
                expect_arity(&args, 2, span)?;
                let url = expect_string(&args[0], span)?;
                let body = expect_string(&args[1], span)?;
                http_client_request("POST", &url, &body, span)
            }
            ("net.http.server", "serve_once") => {
                expect_arity(&args, 3, span)?;
                let host = expect_string(&args[0], span)?;
                let port = expect_int(&args[1], span)?;
                if !(1..=65535).contains(&port) {
                    return Err(IcooError::runtime(
                        "server port must be between 1 and 65535",
                        Some(span),
                    ));
                }
                let body = expect_string(&args[2], span)?;
                http_server_serve_once(&host, port as u16, &body, span)?;
                Ok(Value::Nil)
            }
            ("web.ino", "App") | ("web.ino", "create") => {
                expect_arity(&args, 0, span)?;
                Ok(Value::WebInoApp(Rc::new(RefCell::new(WebInoApp {
                    routes: HashMap::new(),
                }))))
            }
            _ => Err(IcooError::runtime(
                format!(
                    "unknown native module method '{}.{}'",
                    method.module, method.name
                ),
                Some(span),
            )),
        }
    }

    fn call_native_method(
        &mut self,
        method: &NativeMethod,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match method.name.as_str() {
            "to_string" => return Ok(Value::String(method.receiver.display())),
            "type_name" => return Ok(Value::String(method.receiver.type_name())),
            _ => {}
        }

        match &method.receiver {
            Value::String(value) => self.string_method(value, &method.name, args, span),
            Value::Array(values) => self.array_method(values.clone(), &method.name, args, span),
            Value::Map(values) => self.map_method(values.clone(), &method.name, args, span),
            Value::EventLoop(loop_ref) => {
                self.event_loop_method(loop_ref.clone(), &method.name, args, span)
            }
            Value::WebInoApp(app) => self.web_ino_app_method(app.clone(), &method.name, args, span),
            Value::WebInoResponse(response) => {
                self.web_ino_response_method(response.clone(), &method.name, args, span)
            }
            Value::Task(task) => self.task_method(task.clone(), &method.name, args, span),
            Value::Bool(value) if method.name == "to_int" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(i64::from(*value)))
            }
            Value::Int(value) if method.name == "to_float" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Float(*value as f64))
            }
            Value::Int(value) if method.name == "abs" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(value.abs()))
            }
            Value::Float(value) if method.name == "to_int" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(*value as i64))
            }
            Value::Float(value) if method.name == "abs" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Float(value.abs()))
            }
            _ => Err(IcooError::runtime(
                format!(
                    "type '{}' has no native method '{}'",
                    method.receiver.type_name(),
                    method.name
                ),
                Some(span),
            )),
        }
    }

    fn web_ino_app_method(
        &mut self,
        app: Rc<RefCell<WebInoApp>>,
        name: &str,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match name {
            "get" | "post" => {
                expect_arity(&args, 2, span)?;
                let path = expect_string(&args[0], span)?;
                if !path.starts_with('/') {
                    return Err(IcooError::runtime(
                        "route path must start with '/'",
                        Some(span),
                    ));
                }
                if !is_callable(&args[1]) {
                    return Err(IcooError::runtime(
                        "route handler must be callable",
                        Some(span),
                    ));
                }
                let method = name.to_ascii_uppercase();
                app.borrow_mut().routes.insert(
                    web_ino_route_key(&method, &path),
                    WebInoRoute {
                        method,
                        path,
                        handler: args[1].clone(),
                    },
                );
                Ok(Value::WebInoApp(app))
            }
            "listen_once" => {
                expect_arity(&args, 2, span)?;
                let host = expect_string(&args[0], span)?;
                let port = expect_int(&args[1], span)?;
                if !(1..=65535).contains(&port) {
                    return Err(IcooError::runtime(
                        "server port must be between 1 and 65535",
                        Some(span),
                    ));
                }
                self.web_ino_listen_once(app, &host, port as u16, span)?;
                Ok(Value::Nil)
            }
            "listen" => {
                expect_arity(&args, 3, span)?;
                let host = expect_string(&args[0], span)?;
                let port = expect_int(&args[1], span)?;
                let max_requests = expect_int(&args[2], span)?;
                if !(1..=65535).contains(&port) {
                    return Err(IcooError::runtime(
                        "server port must be between 1 and 65535",
                        Some(span),
                    ));
                }
                if max_requests <= 0 {
                    return Err(IcooError::runtime(
                        "max_requests must be positive",
                        Some(span),
                    ));
                }
                let workers = std::thread::available_parallelism()
                    .map(|count| count.get())
                    .unwrap_or(1);
                self.web_ino_listen(
                    app,
                    &host,
                    port as u16,
                    max_requests as usize,
                    workers,
                    span,
                )?;
                Ok(Value::Nil)
            }
            "listen_with_workers" => {
                expect_arity(&args, 4, span)?;
                let host = expect_string(&args[0], span)?;
                let port = expect_int(&args[1], span)?;
                let max_requests = expect_int(&args[2], span)?;
                let workers = expect_int(&args[3], span)?;
                if !(1..=65535).contains(&port) {
                    return Err(IcooError::runtime(
                        "server port must be between 1 and 65535",
                        Some(span),
                    ));
                }
                if max_requests <= 0 {
                    return Err(IcooError::runtime(
                        "max_requests must be positive",
                        Some(span),
                    ));
                }
                if workers <= 0 {
                    return Err(IcooError::runtime("workers must be positive", Some(span)));
                }
                self.web_ino_listen(
                    app,
                    &host,
                    port as u16,
                    max_requests as usize,
                    workers as usize,
                    span,
                )?;
                Ok(Value::Nil)
            }
            _ => Err(IcooError::runtime("unknown WebInoApp method", Some(span))),
        }
    }

    fn web_ino_response_method(
        &mut self,
        response: Rc<RefCell<WebInoResponse>>,
        name: &str,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match name {
            "status" => {
                expect_arity(&args, 1, span)?;
                let status = expect_int(&args[0], span)?;
                if !(100..=999).contains(&status) {
                    return Err(IcooError::runtime(
                        "HTTP status must be between 100 and 999",
                        Some(span),
                    ));
                }
                {
                    let mut response_ref = response.borrow_mut();
                    if response_ref.headers_sent {
                        return Err(IcooError::runtime(
                            "cannot change HTTP status after streaming has started",
                            Some(span),
                        ));
                    }
                    response_ref.status = status;
                }
                Ok(Value::WebInoResponse(response))
            }
            "send" => {
                expect_arity(&args, 1, span)?;
                let mut response_ref = response.borrow_mut();
                if response_ref.headers_sent {
                    return Err(IcooError::runtime(
                        "cannot send response after streaming has started",
                        Some(span),
                    ));
                }
                response_ref.body = args[0].display();
                response_ref.chunks.clear();
                response_ref.content_type = "text/plain; charset=utf-8".to_string();
                response_ref.sent = true;
                response_ref.streaming = false;
                Ok(Value::Nil)
            }
            "json" => {
                expect_arity(&args, 1, span)?;
                let body =
                    serde_json::to_string(&value_to_json(&args[0], span)?).map_err(|err| {
                        IcooError::runtime(
                            format!("web response json() failed: {}", err),
                            Some(span),
                        )
                    })?;
                let mut response_ref = response.borrow_mut();
                if response_ref.headers_sent {
                    return Err(IcooError::runtime(
                        "cannot send JSON response after streaming has started",
                        Some(span),
                    ));
                }
                response_ref.body = body;
                response_ref.chunks.clear();
                response_ref.content_type = "application/json; charset=utf-8".to_string();
                response_ref.sent = true;
                response_ref.streaming = false;
                Ok(Value::Nil)
            }
            "write" => {
                expect_arity(&args, 1, span)?;
                {
                    let mut response_ref = response.borrow_mut();
                    web_ino_write_stream_chunk(&mut response_ref, args[0].display(), span)?;
                }
                Ok(Value::WebInoResponse(response))
            }
            "end" => {
                expect_arity(&args, 0, span)?;
                let mut response_ref = response.borrow_mut();
                if response_ref.streaming {
                    web_ino_end_stream(&mut response_ref, span)?;
                } else {
                    response_ref.sent = true;
                }
                Ok(Value::Nil)
            }
            _ => Err(IcooError::runtime(
                "unknown WebInoResponse method",
                Some(span),
            )),
        }
    }

    fn web_ino_listen_once(
        &mut self,
        app: Rc<RefCell<WebInoApp>>,
        host: &str,
        port: u16,
        span: Span,
    ) -> IcooResult<()> {
        let listener = std::net::TcpListener::bind((host, port)).map_err(|err| {
            IcooError::runtime(
                format!("web.ino listen_once bind failed: {}", err),
                Some(span),
            )
        })?;
        let (mut stream, _) = listener.accept().map_err(|err| {
            IcooError::runtime(
                format!("web.ino listen_once accept failed: {}", err),
                Some(span),
            )
        })?;
        let request_text = read_web_ino_request_text(&mut stream)
            .map_err(|message| IcooError::runtime(message, Some(span)))?;
        self.web_ino_handle_request(app, &request_text, &mut stream, span)
    }

    fn web_ino_listen(
        &mut self,
        app: Rc<RefCell<WebInoApp>>,
        host: &str,
        port: u16,
        max_requests: usize,
        workers: usize,
        span: Span,
    ) -> IcooResult<()> {
        let listener = std::net::TcpListener::bind((host, port)).map_err(|err| {
            IcooError::runtime(format!("web.ino listen bind failed: {}", err), Some(span))
        })?;
        let workers = workers.max(1).min(max_requests);
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let (stream_tx, stream_rx) = std::sync::mpsc::channel();
        let stream_rx = std::sync::Arc::new(std::sync::Mutex::new(stream_rx));
        let accept_result_tx = result_tx.clone();
        let accept_handle = std::thread::spawn(move || {
            for _ in 0..max_requests {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if stream_tx.send(stream).is_err() {
                            let _ = accept_result_tx.send(WebInoAccepted::AcceptError(
                                "web.ino listen worker queue closed".to_string(),
                            ));
                            break;
                        }
                    }
                    Err(err) => {
                        let _ = accept_result_tx.send(WebInoAccepted::AcceptError(format!(
                            "web.ino listen accept failed: {}",
                            err
                        )));
                        break;
                    }
                }
            }
        });
        let mut worker_handles = Vec::new();
        for _ in 0..workers {
            let stream_rx = stream_rx.clone();
            let result_tx = result_tx.clone();
            worker_handles.push(std::thread::spawn(move || loop {
                let stream = {
                    let stream_rx = stream_rx.lock().expect("web.ino worker queue poisoned");
                    stream_rx.recv()
                };
                let Ok(mut stream) = stream else {
                    break;
                };
                let request = stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .map_err(|err| format!("web.ino request read failed: {}", err))
                    .and_then(|_| read_web_ino_request_text(&mut stream));
                let _ = result_tx.send(WebInoAccepted::Request { request, stream });
            }));
        }
        drop(result_tx);

        for _ in 0..max_requests {
            match result_rx.recv().map_err(|err| {
                IcooError::runtime(format!("web.ino listen failed: {}", err), Some(span))
            })? {
                WebInoAccepted::Request {
                    request: Ok(request),
                    mut stream,
                } => self.web_ino_handle_request(app.clone(), &request, &mut stream, span)?,
                WebInoAccepted::Request {
                    request: Err(message),
                    mut stream,
                } => {
                    let response = WebInoResponse {
                        status: 400,
                        body: message,
                        chunks: Vec::new(),
                        content_type: "text/plain; charset=utf-8".to_string(),
                        sent: true,
                        streaming: false,
                        headers_sent: false,
                        stream_ended: false,
                        writer: None,
                    };
                    let response_text = web_ino_http_response(&response);
                    std::io::Write::write_all(&mut stream, response_text.as_bytes()).map_err(
                        |err| {
                            IcooError::runtime(
                                format!("web.ino response write failed: {}", err),
                                Some(span),
                            )
                        },
                    )?;
                }
                WebInoAccepted::AcceptError(message) => {
                    let _ = accept_handle.join();
                    return Err(IcooError::runtime(message, Some(span)));
                }
            }
        }
        accept_handle
            .join()
            .map_err(|_| IcooError::runtime("web.ino listen accept thread panicked", Some(span)))?;
        for worker_handle in worker_handles {
            worker_handle.join().map_err(|_| {
                IcooError::runtime("web.ino listen worker thread panicked", Some(span))
            })?;
        }
        Ok(())
    }

    fn web_ino_handle_request(
        &mut self,
        app: Rc<RefCell<WebInoApp>>,
        request_text: &str,
        stream: &mut std::net::TcpStream,
        span: Span,
    ) -> IcooResult<()> {
        let request = parse_web_ino_request(request_text, span)?;
        let route = app
            .borrow()
            .routes
            .get(&web_ino_route_key(&request.method, &request.path))
            .cloned();
        let writer = stream.try_clone().map_err(|err| {
            IcooError::runtime(
                format!("web.ino response stream clone failed: {}", err),
                Some(span),
            )
        })?;
        let response = Rc::new(RefCell::new(WebInoResponse {
            status: 200,
            body: String::new(),
            chunks: Vec::new(),
            content_type: "text/plain; charset=utf-8".to_string(),
            sent: false,
            streaming: false,
            headers_sent: false,
            stream_ended: false,
            writer: Some(Rc::new(RefCell::new(writer))),
        }));
        if let Some(route) = route {
            let result = self.call_value(
                route.handler,
                vec![
                    web_ino_request_value(&request),
                    Value::WebInoResponse(response.clone()),
                ],
                span,
            )?;
            if !response.borrow().sent && !matches!(result, Value::Nil) {
                let mut response_ref = response.borrow_mut();
                response_ref.body = result.display();
                response_ref.sent = true;
            }
        } else {
            let mut response_ref = response.borrow_mut();
            response_ref.status = 404;
            response_ref.body = "Not Found".to_string();
            response_ref.sent = true;
        }
        if response.borrow().streaming && response.borrow().headers_sent {
            let mut response_ref = response.borrow_mut();
            web_ino_end_stream(&mut response_ref, span)?;
            return Ok(());
        }
        let response_text = web_ino_http_response(&response.borrow());
        std::io::Write::write_all(stream, response_text.as_bytes()).map_err(|err| {
            IcooError::runtime(
                format!("web.ino response write failed: {}", err),
                Some(span),
            )
        })
    }

    fn event_loop_method(
        &mut self,
        loop_ref: Rc<RefCell<IcooEventLoop>>,
        name: &str,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match name {
            "spawn" => {
                expect_arity(&args, 1, span)?;
                let Value::Coroutine(coroutine) = &args[0] else {
                    return Err(IcooError::runtime(
                        "spawn() expects a Coroutine",
                        Some(span),
                    ));
                };
                if coroutine.borrow().owner_task.is_some() {
                    return Err(IcooError::runtime(
                        "coroutine has already been spawned",
                        Some(span),
                    ));
                }
                let task = {
                    let mut event_loop = loop_ref.borrow_mut();
                    let id = event_loop.next_task_id;
                    event_loop.next_task_id += 1;
                    let task = Rc::new(RefCell::new(IcooTask {
                        id,
                        coroutine: coroutine.clone(),
                        state: TaskState::Queued,
                        result: None,
                        error: None,
                        awaiters: Vec::new(),
                    }));
                    coroutine.borrow_mut().owner_task = Some(id);
                    event_loop.ready.push_back(task.clone());
                    task
                };
                Ok(Value::Task(task))
            }
            "run" => {
                expect_arity(&args, 0, span)?;
                self.run_event_loop(loop_ref, span)?;
                Ok(Value::Nil)
            }
            "run_until" => {
                expect_arity(&args, 1, span)?;
                let Value::Task(task) = &args[0] else {
                    return Err(IcooError::runtime("run_until() expects a Task", Some(span)));
                };
                self.run_event_loop(loop_ref, span)?;
                task_result(task.clone(), span)
            }
            "stop" => {
                expect_arity(&args, 0, span)?;
                loop_ref.borrow_mut().stopped = true;
                Ok(Value::Nil)
            }
            "is_stopped" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Bool(loop_ref.borrow().stopped))
            }
            "backend_name" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::String(loop_ref.borrow().backend.name().to_string()))
            }
            "worker_threads" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(loop_ref.borrow().backend.worker_threads() as i64))
            }
            _ => Err(IcooError::runtime("unknown EventLoop method", Some(span))),
        }
    }

    fn task_method(
        &mut self,
        task: Rc<RefCell<IcooTask>>,
        name: &str,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match name {
            "is_done" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Bool(matches!(
                    task.borrow().state,
                    TaskState::Done | TaskState::Failed | TaskState::Cancelled
                )))
            }
            "is_failed" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Bool(task.borrow().state == TaskState::Failed))
            }
            "result" => {
                expect_arity(&args, 0, span)?;
                task_result(task, span)
            }
            "cancel" => {
                expect_arity(&args, 0, span)?;
                let mut task_ref = task.borrow_mut();
                if !matches!(
                    task_ref.state,
                    TaskState::Done | TaskState::Failed | TaskState::Cancelled
                ) {
                    task_ref.state = TaskState::Cancelled;
                }
                Ok(Value::Nil)
            }
            _ => Err(IcooError::runtime("unknown Task method", Some(span))),
        }
    }

    fn run_event_loop(
        &mut self,
        loop_ref: Rc<RefCell<IcooEventLoop>>,
        span: Span,
    ) -> IcooResult<()> {
        loop {
            if loop_ref.borrow().stopped {
                break;
            }
            enqueue_due_timers(&loop_ref);
            let Some(task) = loop_ref.borrow_mut().ready.pop_front() else {
                if wait_for_next_timer(&loop_ref) {
                    continue;
                } else {
                    break;
                }
            };
            self.run_task(loop_ref.clone(), task, span)?;
        }
        Ok(())
    }

    fn run_task(
        &mut self,
        loop_ref: Rc<RefCell<IcooEventLoop>>,
        task: Rc<RefCell<IcooTask>>,
        span: Span,
    ) -> IcooResult<()> {
        if matches!(
            task.borrow().state,
            TaskState::Done | TaskState::Failed | TaskState::Cancelled
        ) {
            return Ok(());
        }
        task.borrow_mut().state = TaskState::Running;

        let previous_loop = self.current_loop.replace(loop_ref.clone());
        let previous_task = self.current_task.replace(task.clone());
        let result = self.run_coroutine_until_pause(task.borrow().coroutine.clone());
        self.current_loop = previous_loop;
        self.current_task = previous_task;

        match result {
            Ok(CoroutineStep::Yielded) => {
                task.borrow_mut().state = TaskState::Queued;
                loop_ref.borrow_mut().ready.push_back(task);
            }
            Ok(CoroutineStep::Done(value)) => {
                complete_task(task, TaskState::Done, Some(value), None, &loop_ref);
            }
            Err(IcooError::Await(Value::Task(waiting_on))) => {
                task.borrow_mut().state = TaskState::Waiting;
                waiting_on.borrow_mut().awaiters.push(task);
            }
            Err(err) => {
                complete_task(
                    task,
                    TaskState::Failed,
                    None,
                    Some(err.to_string()),
                    &loop_ref,
                );
            }
        }
        let _ = span;
        Ok(())
    }

    fn run_coroutine_until_pause(
        &mut self,
        coroutine: Rc<RefCell<IcooCoroutine>>,
    ) -> IcooResult<CoroutineStep> {
        let previous_env = self.env.clone();
        self.env = coroutine.borrow().env.clone();
        let result = loop {
            let instr = {
                let coroutine_ref = coroutine.borrow();
                if coroutine_ref.pc >= coroutine_ref.instructions.len() {
                    if let Some(return_type) = &coroutine_ref.return_type {
                        check_value_type(
                            &Value::Nil,
                            return_type,
                            &format!("return value of '{}'", coroutine_ref.name),
                            return_type.span,
                        )?;
                    }
                    break Ok(CoroutineStep::Done(Value::Nil));
                }
                coroutine_ref.instructions[coroutine_ref.pc].clone()
            };

            match instr {
                CoroutineInstr::Stmt(stmt) => match self.execute(&stmt) {
                    Ok(()) => coroutine.borrow_mut().pc += 1,
                    Err(IcooError::Return(value)) => {
                        if let Some(return_type) = &coroutine.borrow().return_type {
                            let span = match &stmt {
                                Stmt::Return { span, .. } => *span,
                                _ => return_type.span,
                            };
                            check_value_type(
                                &value,
                                return_type,
                                &format!("return value of '{}'", coroutine.borrow().name),
                                span,
                            )?;
                        }
                        break Ok(CoroutineStep::Done(value));
                    }
                    Err(err) => break Err(err),
                },
                CoroutineInstr::JumpIfFalse { condition, target } => {
                    if self.eval(&condition)?.truthy() {
                        coroutine.borrow_mut().pc += 1;
                    } else {
                        coroutine.borrow_mut().pc = target;
                    }
                }
                CoroutineInstr::Jump { target } => {
                    coroutine.borrow_mut().pc = target;
                }
                CoroutineInstr::Yield(value) => {
                    let value = if let Some(value) = value {
                        self.eval(&value)?
                    } else {
                        Value::Nil
                    };
                    coroutine.borrow_mut().pc += 1;
                    let _ = value;
                    break Ok(CoroutineStep::Yielded);
                }
            }
        };
        self.env = previous_env;
        result
    }

    fn string_method(
        &self,
        value: &str,
        name: &str,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match name {
            "len" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(value.chars().count() as i64))
            }
            "is_empty" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Bool(value.is_empty()))
            }
            "contains" => {
                expect_arity(&args, 1, span)?;
                let needle = expect_string(&args[0], span)?;
                Ok(Value::Bool(value.contains(&needle)))
            }
            _ => Err(IcooError::runtime("unknown String method", Some(span))),
        }
    }

    fn array_method(
        &mut self,
        values: Rc<RefCell<Vec<Value>>>,
        name: &str,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match name {
            "len" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(values.borrow().len() as i64))
            }
            "is_empty" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Bool(values.borrow().is_empty()))
            }
            "push" => {
                expect_arity(&args, 1, span)?;
                values.borrow_mut().push(args[0].clone());
                Ok(Value::Nil)
            }
            "pop" => {
                expect_arity(&args, 0, span)?;
                Ok(values.borrow_mut().pop().unwrap_or(Value::Nil))
            }
            "shift" => {
                expect_arity(&args, 0, span)?;
                let mut values = values.borrow_mut();
                if values.is_empty() {
                    Ok(Value::Nil)
                } else {
                    Ok(values.remove(0))
                }
            }
            "unshift" => {
                expect_arity(&args, 1, span)?;
                let mut values_ref = values.borrow_mut();
                values_ref.insert(0, args[0].clone());
                Ok(Value::Int(values_ref.len() as i64))
            }
            "at" => {
                expect_arity(&args, 1, span)?;
                let index = normalize_index(expect_int(&args[0], span)?, values.borrow().len());
                Ok(index
                    .and_then(|index| values.borrow().get(index).cloned())
                    .unwrap_or(Value::Nil))
            }
            "includes" => {
                expect_arity(&args, 1, span)?;
                Ok(Value::Bool(
                    values
                        .borrow()
                        .iter()
                        .any(|value| value_equal(value, &args[0])),
                ))
            }
            "index_of" => {
                expect_arity(&args, 1, span)?;
                let index = values
                    .borrow()
                    .iter()
                    .position(|value| value_equal(value, &args[0]))
                    .map(|index| index as i64)
                    .unwrap_or(-1);
                Ok(Value::Int(index))
            }
            "slice" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(arity_error("slice", "1 or 2", args.len(), span));
                }
                let values_ref = values.borrow();
                let len = values_ref.len() as i64;
                let start = clamp_slice_index(expect_int(&args[0], span)?, len);
                let end = if args.len() == 2 {
                    clamp_slice_index(expect_int(&args[1], span)?, len)
                } else {
                    len
                };
                let result = if end < start {
                    Vec::new()
                } else {
                    values_ref[start as usize..end as usize].to_vec()
                };
                Ok(Value::Array(Rc::new(RefCell::new(result))))
            }
            "splice" => {
                if args.len() < 2 {
                    return Err(arity_error("splice", "at least 2", args.len(), span));
                }
                let mut values_ref = values.borrow_mut();
                let len = values_ref.len() as i64;
                let start = clamp_slice_index(expect_int(&args[0], span)?, len) as usize;
                let delete_count = expect_int(&args[1], span)?.max(0) as usize;
                let delete_count = delete_count.min(values_ref.len().saturating_sub(start));
                let removed: Vec<Value> = values_ref
                    .splice(start..start + delete_count, args.iter().skip(2).cloned())
                    .collect();
                Ok(Value::Array(Rc::new(RefCell::new(removed))))
            }
            "join" => {
                if args.len() > 1 {
                    return Err(arity_error("join", "0 or 1", args.len(), span));
                }
                let separator = if args.is_empty() {
                    ",".to_string()
                } else {
                    expect_string(&args[0], span)?
                };
                let parts: Vec<String> = values.borrow().iter().map(Value::display).collect();
                Ok(Value::String(parts.join(&separator)))
            }
            "reverse" => {
                expect_arity(&args, 0, span)?;
                values.borrow_mut().reverse();
                Ok(Value::Array(values))
            }
            "for_each" => {
                expect_arity(&args, 1, span)?;
                let snapshot = values.borrow().clone();
                for (index, value) in snapshot.into_iter().enumerate() {
                    self.call_value(args[0].clone(), vec![value, Value::Int(index as i64)], span)?;
                }
                Ok(Value::Nil)
            }
            "map" => {
                expect_arity(&args, 1, span)?;
                let snapshot = values.borrow().clone();
                let mut result = Vec::new();
                for (index, value) in snapshot.into_iter().enumerate() {
                    result.push(self.call_value(
                        args[0].clone(),
                        vec![value, Value::Int(index as i64)],
                        span,
                    )?);
                }
                Ok(Value::Array(Rc::new(RefCell::new(result))))
            }
            "filter" => {
                expect_arity(&args, 1, span)?;
                let snapshot = values.borrow().clone();
                let mut result = Vec::new();
                for (index, value) in snapshot.into_iter().enumerate() {
                    if self
                        .call_value(
                            args[0].clone(),
                            vec![value.clone(), Value::Int(index as i64)],
                            span,
                        )?
                        .truthy()
                    {
                        result.push(value);
                    }
                }
                Ok(Value::Array(Rc::new(RefCell::new(result))))
            }
            "reduce" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(arity_error("reduce", "1 or 2", args.len(), span));
                }
                let snapshot = values.borrow().clone();
                let mut iter = snapshot.into_iter().enumerate();
                let mut acc = if args.len() == 2 {
                    args[1].clone()
                } else if let Some((_, first)) = iter.next() {
                    first
                } else {
                    return Ok(Value::Nil);
                };
                for (index, value) in iter {
                    acc = self.call_value(
                        args[0].clone(),
                        vec![acc, value, Value::Int(index as i64)],
                        span,
                    )?;
                }
                Ok(acc)
            }
            "find" => {
                expect_arity(&args, 1, span)?;
                for (index, value) in values.borrow().clone().into_iter().enumerate() {
                    if self
                        .call_value(
                            args[0].clone(),
                            vec![value.clone(), Value::Int(index as i64)],
                            span,
                        )?
                        .truthy()
                    {
                        return Ok(value);
                    }
                }
                Ok(Value::Nil)
            }
            "find_index" => {
                expect_arity(&args, 1, span)?;
                for (index, value) in values.borrow().clone().into_iter().enumerate() {
                    if self
                        .call_value(args[0].clone(), vec![value, Value::Int(index as i64)], span)?
                        .truthy()
                    {
                        return Ok(Value::Int(index as i64));
                    }
                }
                Ok(Value::Int(-1))
            }
            "some" => {
                expect_arity(&args, 1, span)?;
                for (index, value) in values.borrow().clone().into_iter().enumerate() {
                    if self
                        .call_value(args[0].clone(), vec![value, Value::Int(index as i64)], span)?
                        .truthy()
                    {
                        return Ok(Value::Bool(true));
                    }
                }
                Ok(Value::Bool(false))
            }
            "every" => {
                expect_arity(&args, 1, span)?;
                for (index, value) in values.borrow().clone().into_iter().enumerate() {
                    if !self
                        .call_value(args[0].clone(), vec![value, Value::Int(index as i64)], span)?
                        .truthy()
                    {
                        return Ok(Value::Bool(false));
                    }
                }
                Ok(Value::Bool(true))
            }
            _ => Err(IcooError::runtime("unknown Array method", Some(span))),
        }
    }

    fn map_method(
        &mut self,
        values: Rc<RefCell<HashMap<String, Value>>>,
        name: &str,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match name {
            "len" | "size" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(values.borrow().len() as i64))
            }
            "is_empty" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Bool(values.borrow().is_empty()))
            }
            "has" => {
                expect_arity(&args, 1, span)?;
                Ok(Value::Bool(
                    values
                        .borrow()
                        .contains_key(&expect_string(&args[0], span)?),
                ))
            }
            "get" => {
                expect_arity(&args, 1, span)?;
                Ok(values
                    .borrow()
                    .get(&expect_string(&args[0], span)?)
                    .cloned()
                    .unwrap_or(Value::Nil))
            }
            "set" => {
                expect_arity(&args, 2, span)?;
                values
                    .borrow_mut()
                    .insert(expect_string(&args[0], span)?, args[1].clone());
                Ok(Value::Map(values))
            }
            "delete" => {
                expect_arity(&args, 1, span)?;
                Ok(Value::Bool(
                    values
                        .borrow_mut()
                        .remove(&expect_string(&args[0], span)?)
                        .is_some(),
                ))
            }
            "clear" => {
                expect_arity(&args, 0, span)?;
                values.borrow_mut().clear();
                Ok(Value::Nil)
            }
            "keys" => {
                expect_arity(&args, 0, span)?;
                let result = values.borrow().keys().cloned().map(Value::String).collect();
                Ok(Value::Array(Rc::new(RefCell::new(result))))
            }
            "values" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Array(Rc::new(RefCell::new(
                    values.borrow().values().cloned().collect(),
                ))))
            }
            "entries" => {
                expect_arity(&args, 0, span)?;
                let result = values
                    .borrow()
                    .iter()
                    .map(|(key, value)| {
                        Value::Array(Rc::new(RefCell::new(vec![
                            Value::String(key.clone()),
                            value.clone(),
                        ])))
                    })
                    .collect();
                Ok(Value::Array(Rc::new(RefCell::new(result))))
            }
            "for_each" => {
                expect_arity(&args, 1, span)?;
                let snapshot: Vec<(String, Value)> = values
                    .borrow()
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                for (key, value) in snapshot {
                    self.call_value(args[0].clone(), vec![value, Value::String(key)], span)?;
                }
                Ok(Value::Nil)
            }
            _ => Err(IcooError::runtime("unknown Map method", Some(span))),
        }
    }
}

enum CoroutineStep {
    Yielded,
    Done(Value),
}

fn task_result(task: Rc<RefCell<IcooTask>>, span: Span) -> IcooResult<Value> {
    match task.borrow().state {
        TaskState::Done => Ok(task.borrow().result.clone().unwrap_or(Value::Nil)),
        TaskState::Failed => Err(IcooError::runtime(
            task.borrow()
                .error
                .clone()
                .unwrap_or_else(|| "task failed".to_string()),
            Some(span),
        )),
        TaskState::Cancelled => Err(IcooError::runtime("task was cancelled", Some(span))),
        _ => Err(IcooError::runtime("task is not done", Some(span))),
    }
}

fn schedule_sleep_task(loop_ref: Rc<RefCell<IcooEventLoop>>, millis: u64) -> Rc<RefCell<IcooTask>> {
    let env = Environment::new();
    let coroutine = Rc::new(RefCell::new(IcooCoroutine {
        name: "sleep".to_string(),
        return_type: None,
        env,
        instructions: Vec::new(),
        pc: 0,
        owner_task: None,
    }));
    let mut event_loop = loop_ref.borrow_mut();
    let id = event_loop.next_task_id;
    event_loop.next_task_id += 1;
    coroutine.borrow_mut().owner_task = Some(id);
    let task = Rc::new(RefCell::new(IcooTask {
        id,
        coroutine,
        state: TaskState::Queued,
        result: None,
        error: None,
        awaiters: Vec::new(),
    }));
    event_loop.timers.push(SleepTimer {
        due: Instant::now() + Duration::from_millis(millis),
        task: task.clone(),
    });
    task
}

fn enqueue_due_timers(loop_ref: &Rc<RefCell<IcooEventLoop>>) {
    let now = Instant::now();
    let mut ready_timers = Vec::new();
    {
        let mut event_loop = loop_ref.borrow_mut();
        let mut index = 0;
        while index < event_loop.timers.len() {
            if event_loop.timers[index].due <= now {
                ready_timers.push(event_loop.timers.swap_remove(index));
            } else {
                index += 1;
            }
        }
    }
    if !ready_timers.is_empty() {
        let mut event_loop = loop_ref.borrow_mut();
        for timer in ready_timers {
            event_loop.ready.push_back(timer.task);
        }
    }
}

fn wait_for_next_timer(loop_ref: &Rc<RefCell<IcooEventLoop>>) -> bool {
    let Some((due, backend)) = ({
        let event_loop = loop_ref.borrow();
        event_loop
            .timers
            .iter()
            .map(|timer| timer.due)
            .min()
            .map(|due| (due, event_loop.backend.clone()))
    }) else {
        return false;
    };
    let now = Instant::now();
    if due > now {
        backend.sleep_blocking(due.duration_since(now));
    }
    enqueue_due_timers(loop_ref);
    true
}

fn complete_task(
    task: Rc<RefCell<IcooTask>>,
    state: TaskState,
    result: Option<Value>,
    error: Option<String>,
    loop_ref: &Rc<RefCell<IcooEventLoop>>,
) {
    let awaiters = {
        let mut task_ref = task.borrow_mut();
        task_ref.state = state;
        task_ref.result = result;
        task_ref.error = error;
        std::mem::take(&mut task_ref.awaiters)
    };
    let mut event_loop = loop_ref.borrow_mut();
    for awaiter in awaiters {
        if awaiter.borrow().state == TaskState::Waiting {
            awaiter.borrow_mut().state = TaskState::Queued;
            event_loop.ready.push_back(awaiter);
        }
    }
}

fn compile_coroutine_body(statements: &[Stmt]) -> Vec<CoroutineInstr> {
    let mut instructions = Vec::new();
    let mut loop_stack = Vec::new();
    compile_statements(statements, &mut instructions, &mut loop_stack);
    instructions
}

struct LoopTargets {
    continue_target: usize,
    break_jumps: Vec<usize>,
}

fn compile_statements(
    statements: &[Stmt],
    instructions: &mut Vec<CoroutineInstr>,
    loop_stack: &mut Vec<LoopTargets>,
) {
    for stmt in statements {
        compile_statement(stmt, instructions, loop_stack);
    }
}

fn compile_statement(
    stmt: &Stmt,
    instructions: &mut Vec<CoroutineInstr>,
    loop_stack: &mut Vec<LoopTargets>,
) {
    match stmt {
        Stmt::Yield { value, .. } => instructions.push(CoroutineInstr::Yield(value.clone())),
        Stmt::While { condition, body } => {
            let start = instructions.len();
            let jump_if_false = instructions.len();
            instructions.push(CoroutineInstr::JumpIfFalse {
                condition: condition.clone(),
                target: usize::MAX,
            });
            loop_stack.push(LoopTargets {
                continue_target: start,
                break_jumps: Vec::new(),
            });
            compile_statements(body, instructions, loop_stack);
            let loop_targets = loop_stack.pop().expect("loop target stack underflow");
            instructions.push(CoroutineInstr::Jump { target: start });
            let end = instructions.len();
            patch_jump_target(instructions, jump_if_false, end);
            for break_jump in loop_targets.break_jumps {
                patch_jump_target(instructions, break_jump, end);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            elifs,
            else_branch,
        } => {
            let mut end_jumps = Vec::new();
            let first_false_jump = instructions.len();
            instructions.push(CoroutineInstr::JumpIfFalse {
                condition: condition.clone(),
                target: usize::MAX,
            });
            compile_statements(then_branch, instructions, loop_stack);
            end_jumps.push(push_jump_placeholder(instructions));
            let mut previous_false_jump = first_false_jump;

            for (condition, body) in elifs {
                let elif_start = instructions.len();
                patch_jump_target(instructions, previous_false_jump, elif_start);
                previous_false_jump = instructions.len();
                instructions.push(CoroutineInstr::JumpIfFalse {
                    condition: condition.clone(),
                    target: usize::MAX,
                });
                compile_statements(body, instructions, loop_stack);
                end_jumps.push(push_jump_placeholder(instructions));
            }

            let else_start = instructions.len();
            patch_jump_target(instructions, previous_false_jump, else_start);
            if let Some(else_branch) = else_branch {
                compile_statements(else_branch, instructions, loop_stack);
            }
            let end = instructions.len();
            for jump in end_jumps {
                patch_jump_target(instructions, jump, end);
            }
        }
        Stmt::Break(_) => {
            if let Some(targets) = loop_stack.last_mut() {
                let jump = push_jump_placeholder(instructions);
                targets.break_jumps.push(jump);
            } else {
                instructions.push(CoroutineInstr::Stmt(stmt.clone()));
            }
        }
        Stmt::Continue(_) => {
            if let Some(targets) = loop_stack.last() {
                instructions.push(CoroutineInstr::Jump {
                    target: targets.continue_target,
                });
            } else {
                instructions.push(CoroutineInstr::Stmt(stmt.clone()));
            }
        }
        _ => instructions.push(CoroutineInstr::Stmt(stmt.clone())),
    }
}

fn push_jump_placeholder(instructions: &mut Vec<CoroutineInstr>) -> usize {
    let index = instructions.len();
    instructions.push(CoroutineInstr::Jump { target: usize::MAX });
    index
}

fn patch_jump_target(instructions: &mut [CoroutineInstr], index: usize, target: usize) {
    match &mut instructions[index] {
        CoroutineInstr::JumpIfFalse { target: slot, .. }
        | CoroutineInstr::Jump { target: slot } => {
            *slot = target;
        }
        _ => {}
    }
}

fn check_value_type(
    value: &Value,
    type_hint: &TypeRef,
    context: &str,
    span: Span,
) -> IcooResult<()> {
    if value_matches_type(value, &type_hint.name) {
        Ok(())
    } else {
        Err(IcooError::runtime(
            format!(
                "expected {} for {} but got {}",
                type_hint.display_name(),
                context,
                value.type_name()
            ),
            Some(span),
        ))
    }
}

fn value_matches_type(value: &Value, type_name: &str) -> bool {
    match type_name {
        "Any" => true,
        "Nil" => matches!(value, Value::Nil),
        "Bool" => matches!(value, Value::Bool(_)),
        "Int" => matches!(value, Value::Int(_)),
        "Float" => matches!(value, Value::Float(_)),
        "String" => matches!(value, Value::String(_)),
        "Array" => matches!(value, Value::Array(_)),
        "Map" => matches!(value, Value::Map(_)),
        "Function" => matches!(
            value,
            Value::Function(_) | Value::NativeFunction(_) | Value::NativeMethod(_)
        ),
        "Coroutine" => matches!(value, Value::Coroutine(_)),
        "Task" => matches!(value, Value::Task(_)),
        "EventLoop" => matches!(value, Value::EventLoop(_)),
        "WebInoApp" => matches!(value, Value::WebInoApp(_)),
        "WebInoResponse" => matches!(value, Value::WebInoResponse(_)),
        class_name => matches_instance_type(value, class_name),
    }
}

fn is_callable(value: &Value) -> bool {
    matches!(
        value,
        Value::Function(_)
            | Value::NativeFunction(_)
            | Value::NativeMethod(_)
            | Value::NativeModuleMethod(_)
    )
}

fn matches_instance_type(value: &Value, class_name: &str) -> bool {
    let Value::Instance(instance) = value else {
        return false;
    };
    let mut class = Some(instance.borrow().class.clone());
    while let Some(current) = class {
        if current.name == class_name {
            return true;
        }
        class = current.superclass.clone();
    }
    false
}

fn numeric(
    left: Value,
    right: Value,
    span: Span,
    int_op: impl Fn(i64, i64) -> i64,
    float_op: impl Fn(f64, f64) -> f64,
) -> IcooResult<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(int_op(a, b))),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(float_op(a, b))),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(float_op(a as f64, b))),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(float_op(a, b as f64))),
        _ => Err(IcooError::runtime("operands must be numbers", Some(span))),
    }
}

fn numeric_float(
    left: Value,
    right: Value,
    span: Span,
    float_op: impl Fn(f64, f64) -> f64,
) -> IcooResult<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Float(float_op(a as f64, b as f64))),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(float_op(a, b))),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(float_op(a as f64, b))),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(float_op(a, b as f64))),
        _ => Err(IcooError::runtime("operands must be numbers", Some(span))),
    }
}

fn compare(
    left: Value,
    right: Value,
    span: Span,
    op: impl Fn(f64, f64) -> bool,
) -> IcooResult<Value> {
    let left = number_as_f64(left, span)?;
    let right = number_as_f64(right, span)?;
    Ok(Value::Bool(op(left, right)))
}

fn number_as_f64(value: Value, span: Span) -> IcooResult<f64> {
    match value {
        Value::Int(value) => Ok(value as f64),
        Value::Float(value) => Ok(value),
        _ => Err(IcooError::runtime("operand must be a number", Some(span))),
    }
}

fn value_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Nil, Value::Nil) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => a == b,
        (Value::Int(a), Value::Float(b)) => (*a as f64) == *b,
        (Value::Float(a), Value::Int(b)) => *a == (*b as f64),
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Array(a), Value::Array(b)) => Rc::ptr_eq(a, b),
        (Value::Map(a), Value::Map(b)) => Rc::ptr_eq(a, b),
        (Value::Coroutine(a), Value::Coroutine(b)) => Rc::ptr_eq(a, b),
        (Value::Task(a), Value::Task(b)) => Rc::ptr_eq(a, b),
        (Value::EventLoop(a), Value::EventLoop(b)) => Rc::ptr_eq(a, b),
        (Value::WebInoApp(a), Value::WebInoApp(b)) => Rc::ptr_eq(a, b),
        (Value::WebInoResponse(a), Value::WebInoResponse(b)) => Rc::ptr_eq(a, b),
        (Value::Instance(a), Value::Instance(b)) => Rc::ptr_eq(a, b),
        (Value::Class(a), Value::Class(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

fn expect_arity(args: &[Value], expected: usize, span: Span) -> IcooResult<()> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(IcooError::runtime(
            format!("expected {} arguments but got {}", expected, args.len()),
            Some(span),
        ))
    }
}

fn arity_error(name: &str, expected: &str, got: usize, span: Span) -> IcooError {
    IcooError::runtime(
        format!(
            "method '{}' expected {} arguments but got {}",
            name, expected, got
        ),
        Some(span),
    )
}

fn expect_string(value: &Value, span: Span) -> IcooResult<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        _ => Err(IcooError::runtime("expected String argument", Some(span))),
    }
}

fn expect_int(value: &Value, span: Span) -> IcooResult<i64> {
    match value {
        Value::Int(value) => Ok(*value),
        _ => Err(IcooError::runtime("expected Int argument", Some(span))),
    }
}

fn expect_number(value: &Value, span: Span) -> IcooResult<f64> {
    match value {
        Value::Int(value) => Ok(*value as f64),
        Value::Float(value) => Ok(*value),
        _ => Err(IcooError::runtime("expected numeric argument", Some(span))),
    }
}

fn numeric_min_max(
    left: &Value,
    right: &Value,
    span: Span,
    op: impl Fn(f64, f64) -> f64,
) -> IcooResult<Value> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => {
            Ok(Value::Int(op(*left as f64, *right as f64) as i64))
        }
        _ => Ok(Value::Float(op(
            expect_number(left, span)?,
            expect_number(right, span)?,
        ))),
    }
}

fn now_duration(span: Span) -> IcooResult<Duration> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| IcooError::runtime(format!("system time error: {}", err), Some(span)))
}

fn has_native_module_method(module: &str, name: &str) -> bool {
    match native_module_kind(module) {
        "math" => matches!(
            name,
            "abs" | "floor" | "ceil" | "round" | "min" | "max" | "random"
        ),
        "time" => matches!(name, "now_ms" | "now_sec"),
        "json" => matches!(name, "stringify" | "parse"),
        "yaml" => matches!(name, "stringify" | "parse"),
        "toml" => matches!(name, "stringify" | "parse"),
        "env" => matches!(name, "cwd" | "args" | "get" | "has"),
        "io" => matches!(name, "print"),
        "io.fs" => matches!(
            name,
            "exists"
                | "is_file"
                | "is_dir"
                | "read_text"
                | "write_text"
                | "append_text"
                | "list_dir"
        ),
        "os" => matches!(
            name,
            "name"
                | "family"
                | "arch"
                | "pid"
                | "cwd"
                | "args"
                | "exe_path"
                | "get_env"
                | "has_env"
        ),
        "net.http.client" => matches!(name, "get" | "post"),
        "net.http.server" => matches!(name, "serve_once"),
        "web.ino" => matches!(name, "App" | "create"),
        _ => false,
    }
}

fn canonical_module_path(path: &Path) -> Result<PathBuf, String> {
    let path = path
        .canonicalize()
        .map_err(|err| format!("failed to resolve '{}': {}", path.display(), err))?;
    if path.extension().and_then(|ext| ext.to_str()) != Some("icoo") {
        return Err(format!(
            "module path '{}' must end with .icoo",
            path.display()
        ));
    }
    Ok(path)
}

fn importable_native_module_name(source: &str) -> Option<&'static str> {
    match source {
        "std.math" => Some("std.math"),
        "std.time" => Some("std.time"),
        "std.json" => Some("std.json"),
        "std.yaml" => Some("std.yaml"),
        "std.toml" => Some("std.toml"),
        "std.env" => Some("std.env"),
        "std.io" => Some("std.io"),
        "std.io.fs" => Some("std.io.fs"),
        "std.os" => Some("std.os"),
        "std.net.http.client" => Some("std.net.http.client"),
        "std.net.http.server" => Some("std.net.http.server"),
        "std.web.ino" => Some("std.web.ino"),
        _ => None,
    }
}

fn native_module_kind(module: &str) -> &str {
    module.strip_prefix("std.").unwrap_or(module)
}

fn imported_member(module: &Value, name: &str, span: Span) -> IcooResult<Value> {
    match module {
        Value::Module(module) => module.exports.get(name).cloned().ok_or_else(|| {
            IcooError::runtime(
                format!(
                    "module '{}' has no export '{}'",
                    module.path.display(),
                    name
                ),
                Some(span),
            )
        }),
        Value::NativeModule(module) if has_native_module_method(&module.name, name) => {
            Ok(Value::NativeModuleMethod(Rc::new(NativeModuleMethod {
                module: module.name.clone(),
                name: name.to_string(),
            })))
        }
        Value::NativeModule(module) => Err(IcooError::runtime(
            format!("module '{}' has no export '{}'", module.name, name),
            Some(span),
        )),
        _ => Err(IcooError::runtime("value is not a module", Some(span))),
    }
}

fn export_name(stmt: &Stmt) -> Option<(String, Span)> {
    match stmt {
        Stmt::Let(decl) | Stmt::Const(decl) | Stmt::Final(decl) => {
            Some((decl.name.name.clone(), decl.name.span))
        }
        Stmt::Function(decl) => Some((decl.name.name.clone(), decl.name.span)),
        Stmt::Class(decl) => Some((decl.name.name.clone(), decl.name.span)),
        _ => None,
    }
}

struct ParsedHttpUrl {
    host: String,
    port: u16,
    path: String,
}

fn parse_http_url(url: &str, span: Span) -> IcooResult<ParsedHttpUrl> {
    let Some(rest) = url.strip_prefix("http://") else {
        return Err(IcooError::runtime(
            "only http:// URLs are supported",
            Some(span),
        ));
    };
    let (host_port, path) = rest
        .split_once('/')
        .map(|(host, path)| (host, format!("/{}", path)))
        .unwrap_or((rest, "/".to_string()));
    if host_port.is_empty() {
        return Err(IcooError::runtime("URL host is required", Some(span)));
    }
    let (host, port) = if let Some((host, port)) = host_port.rsplit_once(':') {
        if host.is_empty() {
            return Err(IcooError::runtime("URL host is required", Some(span)));
        }
        let port = port
            .parse::<u16>()
            .map_err(|_| IcooError::runtime("URL port must be between 1 and 65535", Some(span)))?;
        (host.to_string(), port)
    } else {
        (host_port.to_string(), 80)
    };
    Ok(ParsedHttpUrl { host, port, path })
}

fn http_client_request(method: &str, url: &str, body: &str, span: Span) -> IcooResult<Value> {
    let parsed = parse_http_url(url, span)?;
    let mut stream =
        std::net::TcpStream::connect((parsed.host.as_str(), parsed.port)).map_err(|err| {
            IcooError::runtime(
                format!("http client connection failed: {}", err),
                Some(span),
            )
        })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|err| IcooError::runtime(format!("http client failed: {}", err), Some(span)))?;
    let request = if method == "POST" {
        format!(
            "POST {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}",
            parsed.path,
            parsed.host,
            body.as_bytes().len(),
            body
        )
    } else {
        format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            parsed.path, parsed.host
        )
    };
    std::io::Write::write_all(&mut stream, request.as_bytes()).map_err(|err| {
        IcooError::runtime(format!("http client write failed: {}", err), Some(span))
    })?;
    let mut response = String::new();
    std::io::Read::read_to_string(&mut stream, &mut response).map_err(|err| {
        IcooError::runtime(format!("http client read failed: {}", err), Some(span))
    })?;
    parse_http_response(&response, span)
}

fn parse_http_response(response: &str, span: Span) -> IcooResult<Value> {
    let (head, body) = response.split_once("\r\n\r\n").ok_or_else(|| {
        IcooError::runtime(
            "invalid HTTP response: missing header terminator",
            Some(span),
        )
    })?;
    let mut lines = head.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| IcooError::runtime("invalid HTTP response: missing status", Some(span)))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| IcooError::runtime("invalid HTTP response status", Some(span)))?
        .parse::<i64>()
        .map_err(|_| IcooError::runtime("invalid HTTP response status", Some(span)))?;
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(
                name.trim().to_ascii_lowercase(),
                Value::String(value.trim().to_string()),
            );
        }
    }
    let body = if matches!(
        headers.get("transfer-encoding"),
        Some(Value::String(value)) if value.eq_ignore_ascii_case("chunked")
    ) {
        decode_chunked_body(body, span)?
    } else {
        body.to_string()
    };
    let mut result = HashMap::new();
    result.insert("status".to_string(), Value::Int(status));
    result.insert("body".to_string(), Value::String(body));
    result.insert(
        "headers".to_string(),
        Value::Map(Rc::new(RefCell::new(headers))),
    );
    Ok(Value::Map(Rc::new(RefCell::new(result))))
}

fn decode_chunked_body(body: &str, span: Span) -> IcooResult<String> {
    let bytes = body.as_bytes();
    let mut index = 0;
    let mut decoded = Vec::new();
    loop {
        let Some(line_end) = find_crlf(bytes, index) else {
            return Err(IcooError::runtime(
                "invalid chunked response: missing chunk size",
                Some(span),
            ));
        };
        let size_text = std::str::from_utf8(&bytes[index..line_end])
            .map_err(|_| IcooError::runtime("invalid chunked response size", Some(span)))?;
        let size = usize::from_str_radix(size_text.trim(), 16)
            .map_err(|_| IcooError::runtime("invalid chunked response size", Some(span)))?;
        index = line_end + 2;
        if size == 0 {
            break;
        }
        if bytes.len() < index + size + 2 {
            return Err(IcooError::runtime(
                "invalid chunked response: incomplete chunk",
                Some(span),
            ));
        }
        decoded.extend_from_slice(&bytes[index..index + size]);
        index += size;
        if bytes.get(index..index + 2) != Some(b"\r\n") {
            return Err(IcooError::runtime(
                "invalid chunked response: missing chunk terminator",
                Some(span),
            ));
        }
        index += 2;
    }
    Ok(String::from_utf8_lossy(&decoded).into_owned())
}

fn find_crlf(bytes: &[u8], start: usize) -> Option<usize> {
    bytes
        .get(start..)?
        .windows(2)
        .position(|window| window == b"\r\n")
        .map(|offset| start + offset)
}

fn http_server_serve_once(host: &str, port: u16, body: &str, span: Span) -> IcooResult<()> {
    let listener = std::net::TcpListener::bind((host, port)).map_err(|err| {
        IcooError::runtime(format!("http server bind failed: {}", err), Some(span))
    })?;
    let (mut stream, _) = listener.accept().map_err(|err| {
        IcooError::runtime(format!("http server accept failed: {}", err), Some(span))
    })?;
    let mut buffer = [0_u8; 1024];
    let _ = std::io::Read::read(&mut stream, &mut buffer);
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\n\r\n{}",
        body.as_bytes().len(),
        body
    );
    std::io::Write::write_all(&mut stream, response.as_bytes())
        .map_err(|err| IcooError::runtime(format!("http server write failed: {}", err), Some(span)))
}

#[derive(Debug)]
struct ParsedWebInoRequest {
    method: String,
    path: String,
    query: String,
    headers: HashMap<String, Value>,
    body: String,
    form: HashMap<String, Value>,
    files: HashMap<String, Value>,
}

enum WebInoAccepted {
    Request {
        request: Result<String, String>,
        stream: std::net::TcpStream,
    },
    AcceptError(String),
}

fn read_web_ino_request_text(stream: &mut std::net::TcpStream) -> Result<String, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|err| format!("web.ino request read failed: {}", err))?;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let size = std::io::Read::read(stream, &mut buffer)
            .map_err(|err| format!("web.ino request read failed: {}", err))?;
        if size == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..size]);
        let Some(body_start) = find_http_body_start(&bytes) else {
            continue;
        };
        let head = String::from_utf8_lossy(&bytes[..body_start]);
        let content_length = http_content_length(&head);
        if content_length
            .map(|length| bytes.len() >= body_start + length)
            .unwrap_or(true)
        {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn find_http_body_start(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

fn http_content_length(head: &str) -> Option<usize> {
    head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            value.trim().parse::<usize>().ok()
        } else {
            None
        }
    })
}

fn parse_web_ino_request(request: &str, span: Span) -> IcooResult<ParsedWebInoRequest> {
    let (head, body) = request.split_once("\r\n\r\n").unwrap_or((request, ""));
    let mut lines = head.lines();
    let request_line = lines.next().ok_or_else(|| {
        IcooError::runtime("invalid HTTP request: missing request line", Some(span))
    })?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| IcooError::runtime("invalid HTTP request method", Some(span)))?
        .to_ascii_uppercase();
    let target = parts
        .next()
        .ok_or_else(|| IcooError::runtime("invalid HTTP request path", Some(span)))?;
    let (path, query) = target
        .split_once('?')
        .map(|(path, query)| (path.to_string(), query.to_string()))
        .unwrap_or((target.to_string(), String::new()));
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(
                name.trim().to_ascii_lowercase(),
                Value::String(value.trim().to_string()),
            );
        }
    }
    let (form, files) = parse_web_ino_multipart(&headers, body);
    Ok(ParsedWebInoRequest {
        method,
        path,
        query,
        headers,
        body: body.to_string(),
        form,
        files,
    })
}

fn web_ino_request_value(request: &ParsedWebInoRequest) -> Value {
    let mut map = HashMap::new();
    map.insert("method".to_string(), Value::String(request.method.clone()));
    map.insert("path".to_string(), Value::String(request.path.clone()));
    map.insert("query".to_string(), Value::String(request.query.clone()));
    map.insert(
        "headers".to_string(),
        Value::Map(Rc::new(RefCell::new(request.headers.clone()))),
    );
    map.insert("body".to_string(), Value::String(request.body.clone()));
    map.insert(
        "form".to_string(),
        Value::Map(Rc::new(RefCell::new(request.form.clone()))),
    );
    map.insert(
        "files".to_string(),
        Value::Map(Rc::new(RefCell::new(request.files.clone()))),
    );
    Value::Map(Rc::new(RefCell::new(map)))
}

fn parse_web_ino_multipart(
    headers: &HashMap<String, Value>,
    body: &str,
) -> (HashMap<String, Value>, HashMap<String, Value>) {
    let mut form = HashMap::new();
    let mut files = HashMap::new();
    let Some(Value::String(content_type)) = headers.get("content-type") else {
        return (form, files);
    };
    let Some(boundary) = multipart_boundary(content_type) else {
        return (form, files);
    };
    let marker = format!("--{}", boundary);
    for part in body.split(&marker).skip(1) {
        let part = part.trim_start_matches("\r\n");
        if part.starts_with("--") {
            break;
        }
        let Some((part_head, part_body)) = part.split_once("\r\n\r\n") else {
            continue;
        };
        let mut disposition = HashMap::new();
        let mut content_type = "application/octet-stream".to_string();
        for line in part_head.lines() {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            if name.trim().eq_ignore_ascii_case("content-disposition") {
                disposition = parse_header_parameters(value);
            } else if name.trim().eq_ignore_ascii_case("content-type") {
                content_type = value.trim().to_string();
            }
        }
        let Some(field_name) = disposition.get("name").cloned() else {
            continue;
        };
        let content = part_body
            .strip_suffix("\r\n")
            .unwrap_or(part_body)
            .to_string();
        if let Some(filename) = disposition.get("filename").cloned() {
            let mut file = HashMap::new();
            file.insert("field".to_string(), Value::String(field_name.clone()));
            file.insert("filename".to_string(), Value::String(filename));
            file.insert("content_type".to_string(), Value::String(content_type));
            file.insert("content".to_string(), Value::String(content.clone()));
            file.insert(
                "size".to_string(),
                Value::Int(content.as_bytes().len() as i64),
            );
            files.insert(field_name, Value::Map(Rc::new(RefCell::new(file))));
        } else {
            form.insert(field_name, Value::String(content));
        }
    }
    (form, files)
}

fn multipart_boundary(content_type: &str) -> Option<String> {
    let mut parts = content_type.split(';');
    let media_type = parts.next()?.trim();
    if !media_type.eq_ignore_ascii_case("multipart/form-data") {
        return None;
    }
    parts.find_map(|part| {
        let (name, value) = part.split_once('=')?;
        if name.trim().eq_ignore_ascii_case("boundary") {
            Some(trim_header_quotes(value.trim()).to_string())
        } else {
            None
        }
    })
}

fn parse_header_parameters(value: &str) -> HashMap<String, String> {
    value
        .split(';')
        .filter_map(|part| {
            let (name, value) = part.split_once('=')?;
            Some((
                name.trim().to_ascii_lowercase(),
                trim_header_quotes(value.trim()).to_string(),
            ))
        })
        .collect()
}

fn trim_header_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}

fn web_ino_route_key(method: &str, path: &str) -> String {
    let mut key = String::with_capacity(method.len() + path.len() + 1);
    key.push_str(method);
    key.push(' ');
    key.push_str(path);
    key
}

fn web_ino_write_stream_chunk(
    response: &mut WebInoResponse,
    chunk: String,
    span: Span,
) -> IcooResult<()> {
    if response.stream_ended {
        return Err(IcooError::runtime(
            "cannot write after response stream ended",
            Some(span),
        ));
    }
    response.streaming = true;
    response.sent = true;
    if !response.headers_sent {
        web_ino_write_stream_headers(response, span)?;
    }
    if let Some(writer) = response.writer.clone() {
        let frame = format!("{:X}\r\n{}\r\n", chunk.as_bytes().len(), chunk);
        std::io::Write::write_all(&mut *writer.borrow_mut(), frame.as_bytes()).map_err(|err| {
            IcooError::runtime(
                format!("web.ino stream response write failed: {}", err),
                Some(span),
            )
        })?;
    } else {
        response.chunks.push(chunk);
    }
    Ok(())
}

fn web_ino_write_stream_headers(response: &mut WebInoResponse, span: Span) -> IcooResult<()> {
    let status_text = http_status_text(response.status);
    let headers = format!(
        "HTTP/1.1 {} {}\r\nTransfer-Encoding: chunked\r\nContent-Type: {}\r\nConnection: close\r\n\r\n",
        response.status, status_text, response.content_type
    );
    if let Some(writer) = response.writer.clone() {
        std::io::Write::write_all(&mut *writer.borrow_mut(), headers.as_bytes()).map_err(
            |err| {
                IcooError::runtime(
                    format!("web.ino stream response header write failed: {}", err),
                    Some(span),
                )
            },
        )?;
    }
    response.headers_sent = true;
    Ok(())
}

fn web_ino_end_stream(response: &mut WebInoResponse, span: Span) -> IcooResult<()> {
    if response.stream_ended {
        return Ok(());
    }
    response.streaming = true;
    response.sent = true;
    if !response.headers_sent {
        web_ino_write_stream_headers(response, span)?;
    }
    if let Some(writer) = response.writer.clone() {
        std::io::Write::write_all(&mut *writer.borrow_mut(), b"0\r\n\r\n").map_err(|err| {
            IcooError::runtime(
                format!("web.ino stream response end failed: {}", err),
                Some(span),
            )
        })?;
    }
    response.stream_ended = true;
    Ok(())
}

fn web_ino_http_response(response: &WebInoResponse) -> String {
    let body = if response.streaming {
        response.chunks.join("")
    } else {
        response.body.clone()
    };
    let status_text = http_status_text(response.status);
    format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nContent-Type: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        status_text,
        body.as_bytes().len(),
        response.content_type,
        body
    )
}

fn http_status_text(status: i64) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    }
}

fn value_to_json(value: &Value, span: Span) -> IcooResult<serde_json::Value> {
    match value {
        Value::Nil => Ok(serde_json::Value::Null),
        Value::Bool(value) => Ok(serde_json::Value::Bool(*value)),
        Value::Int(value) => Ok(serde_json::Value::Number((*value).into())),
        Value::Float(value) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| {
                IcooError::runtime("Float value cannot be represented as JSON", Some(span))
            }),
        Value::String(value) => Ok(serde_json::Value::String(value.clone())),
        Value::Array(values) => values
            .borrow()
            .iter()
            .map(|value| value_to_json(value, span))
            .collect::<IcooResult<Vec<_>>>()
            .map(serde_json::Value::Array),
        Value::Map(values) => {
            let mut object = serde_json::Map::new();
            let values_ref = values.borrow();
            for (key, value) in values_ref.iter() {
                object.insert(key.clone(), value_to_json(value, span)?);
            }
            Ok(serde_json::Value::Object(object))
        }
        _ => Err(IcooError::runtime(
            format!("type '{}' cannot be represented as JSON", value.type_name()),
            Some(span),
        )),
    }
}

fn json_to_value(value: serde_json::Value, span: Span) -> IcooResult<Value> {
    match value {
        serde_json::Value::Null => Ok(Value::Nil),
        serde_json::Value::Bool(value) => Ok(Value::Bool(value)),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(Value::Int(value))
            } else if let Some(value) = value.as_f64() {
                Ok(Value::Float(value))
            } else {
                Err(IcooError::runtime(
                    "JSON number cannot be represented as Int or Float",
                    Some(span),
                ))
            }
        }
        serde_json::Value::String(value) => Ok(Value::String(value)),
        serde_json::Value::Array(values) => values
            .into_iter()
            .map(|value| json_to_value(value, span))
            .collect::<IcooResult<Vec<_>>>()
            .map(|values| Value::Array(Rc::new(RefCell::new(values)))),
        serde_json::Value::Object(values) => {
            let mut result = HashMap::new();
            for (key, value) in values {
                result.insert(key, json_to_value(value, span)?);
            }
            Ok(Value::Map(Rc::new(RefCell::new(result))))
        }
    }
}

fn value_to_toml(value: &Value, span: Span) -> IcooResult<toml::Value> {
    match value {
        Value::Bool(value) => Ok(toml::Value::Boolean(*value)),
        Value::Int(value) => Ok(toml::Value::Integer(*value)),
        Value::Float(value) if value.is_finite() => Ok(toml::Value::Float(*value)),
        Value::Float(_) => Err(IcooError::runtime(
            "Float value cannot be represented as TOML",
            Some(span),
        )),
        Value::String(value) => Ok(toml::Value::String(value.clone())),
        Value::Array(values) => values
            .borrow()
            .iter()
            .map(|value| value_to_toml(value, span))
            .collect::<IcooResult<Vec<_>>>()
            .map(toml::Value::Array),
        Value::Map(values) => {
            let mut table = toml::map::Map::new();
            let values_ref = values.borrow();
            for (key, value) in values_ref.iter() {
                table.insert(key.clone(), value_to_toml(value, span)?);
            }
            Ok(toml::Value::Table(table))
        }
        Value::Nil => Err(IcooError::runtime(
            "Nil cannot be represented as TOML",
            Some(span),
        )),
        _ => Err(IcooError::runtime(
            format!("type '{}' cannot be represented as TOML", value.type_name()),
            Some(span),
        )),
    }
}

fn toml_to_value(value: toml::Value, span: Span) -> IcooResult<Value> {
    match value {
        toml::Value::String(value) => Ok(Value::String(value)),
        toml::Value::Integer(value) => Ok(Value::Int(value)),
        toml::Value::Float(value) => Ok(Value::Float(value)),
        toml::Value::Boolean(value) => Ok(Value::Bool(value)),
        toml::Value::Datetime(value) => Ok(Value::String(value.to_string())),
        toml::Value::Array(values) => values
            .into_iter()
            .map(|value| toml_to_value(value, span))
            .collect::<IcooResult<Vec<_>>>()
            .map(|values| Value::Array(Rc::new(RefCell::new(values)))),
        toml::Value::Table(values) => {
            let mut result = HashMap::new();
            for (key, value) in values {
                result.insert(key, toml_to_value(value, span)?);
            }
            Ok(Value::Map(Rc::new(RefCell::new(result))))
        }
    }
}

fn normalize_index(index: i64, len: usize) -> Option<usize> {
    let len = len as i64;
    let index = if index < 0 { len + index } else { index };
    if index < 0 || index >= len {
        None
    } else {
        Some(index as usize)
    }
}

fn clamp_slice_index(index: i64, len: i64) -> i64 {
    let index = if index < 0 { len + index } else { index };
    index.clamp(0, len)
}
