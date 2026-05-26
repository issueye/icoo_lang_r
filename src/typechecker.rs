use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use crate::native_modules;
use crate::parser::ast::*;
use std::collections::HashMap;

pub fn check(program: &Program) -> IcooResult<()> {
    TypeChecker::new(program).check_program(program)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TypeInfo {
    Known(String),
    Array(Box<TypeInfo>),
    Map(Box<TypeInfo>, Box<TypeInfo>),
    Coroutine(Box<TypeInfo>),
    Task(Box<TypeInfo>),
    Unknown,
}

impl TypeInfo {
    fn known(name: impl Into<String>) -> Self {
        Self::Known(name.into())
    }

    fn name(&self) -> Option<&str> {
        match self {
            TypeInfo::Known(name) => Some(name),
            TypeInfo::Array(_) => Some("Array"),
            TypeInfo::Map(_, _) => Some("Map"),
            TypeInfo::Coroutine(_) => Some("Coroutine"),
            TypeInfo::Task(_) => Some("Task"),
            TypeInfo::Unknown => None,
        }
    }

    fn coroutine(result: TypeInfo) -> Self {
        Self::Coroutine(Box::new(result))
    }

    fn array(item: TypeInfo) -> Self {
        Self::Array(Box::new(item))
    }

    fn map(key: TypeInfo, value: TypeInfo) -> Self {
        Self::Map(Box::new(key), Box::new(value))
    }

    fn task(result: TypeInfo) -> Self {
        Self::Task(Box::new(result))
    }

    fn display_name(&self) -> String {
        match self {
            TypeInfo::Known(name) => name.clone(),
            TypeInfo::Array(item) => format!("Array<{}>", item.display_name()),
            TypeInfo::Map(key, value) => {
                format!("Map<{}, {}>", key.display_name(), value.display_name())
            }
            TypeInfo::Coroutine(result) => format!("Coroutine<{}>", result.display_name()),
            TypeInfo::Task(result) => format!("Task<{}>", result.display_name()),
            TypeInfo::Unknown => "Unknown".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
struct FunctionSig {
    params: Vec<Option<TypeInfo>>,
    return_type: TypeInfo,
    is_async: bool,
}

#[derive(Debug, Clone)]
struct ClassInfo {
    superclass: Option<String>,
    fields: HashMap<String, TypeInfo>,
    methods: HashMap<String, FunctionSig>,
}

#[derive(Debug, Clone, Copy)]
enum NativeArity {
    Exact(usize),
    Range { min: usize, max: usize },
    AtLeast(usize),
}

#[derive(Debug, Clone)]
struct NativeMethodSig {
    arity: NativeArity,
    params: Vec<Option<TypeInfo>>,
    variadic: Option<TypeInfo>,
    return_type: TypeInfo,
}

struct TypeChecker {
    globals: HashMap<String, TypeInfo>,
    functions: HashMap<String, FunctionSig>,
    classes: HashMap<String, ClassInfo>,
    scopes: Vec<HashMap<String, TypeInfo>>,
    current_return: Option<TypeInfo>,
    current_class: Option<String>,
}

impl TypeChecker {
    fn new(program: &Program) -> Self {
        let mut checker = Self {
            globals: native_globals(),
            functions: native_functions(),
            classes: HashMap::new(),
            scopes: Vec::new(),
            current_return: None,
            current_class: None,
        };
        checker.collect_declarations(program);
        checker
    }

    fn collect_declarations(&mut self, program: &Program) {
        for stmt in &program.statements {
            match stmt {
                Stmt::ExportDecl(inner) => match inner.as_ref() {
                    Stmt::Function(decl) => {
                        self.functions
                            .insert(decl.name.name.clone(), function_sig(decl));
                        self.globals
                            .insert(decl.name.name.clone(), TypeInfo::known("Function"));
                    }
                    Stmt::Class(decl) => {
                        let mut fields = HashMap::new();
                        for field in &decl.fields {
                            fields.insert(field.name.name.clone(), type_from_ref(&field.type_hint));
                        }
                        let mut methods = HashMap::new();
                        for method in &decl.methods {
                            methods.insert(method.name.name.clone(), function_sig(method));
                        }
                        self.classes.insert(
                            decl.name.name.clone(),
                            ClassInfo {
                                superclass: decl.superclass.as_ref().map(|name| name.name.clone()),
                                fields,
                                methods,
                            },
                        );
                        self.globals
                            .insert(decl.name.name.clone(), TypeInfo::known("Class"));
                    }
                    _ => {}
                },
                Stmt::Function(decl) => {
                    self.functions
                        .insert(decl.name.name.clone(), function_sig(decl));
                    self.globals
                        .insert(decl.name.name.clone(), TypeInfo::known("Function"));
                }
                Stmt::Class(decl) => {
                    let mut fields = HashMap::new();
                    for field in &decl.fields {
                        fields.insert(field.name.name.clone(), type_from_ref(&field.type_hint));
                    }
                    let mut methods = HashMap::new();
                    for method in &decl.methods {
                        methods.insert(method.name.name.clone(), function_sig(method));
                    }
                    self.classes.insert(
                        decl.name.name.clone(),
                        ClassInfo {
                            superclass: decl.superclass.as_ref().map(|name| name.name.clone()),
                            fields,
                            methods,
                        },
                    );
                    self.globals
                        .insert(decl.name.name.clone(), TypeInfo::known("Class"));
                }
                _ => {}
            }
        }
    }

    fn check_program(&mut self, program: &Program) -> IcooResult<()> {
        self.push_scope();
        for stmt in &program.statements {
            self.check_stmt(stmt)?;
        }
        self.pop_scope();
        Ok(())
    }

    fn check_stmt(&mut self, stmt: &Stmt) -> IcooResult<()> {
        match stmt {
            Stmt::ImportModule { source, alias, .. } => {
                self.define(alias.name.clone(), import_module_type(source));
            }
            Stmt::ImportNames { items, .. } => {
                for item in items {
                    let local = item.alias.as_ref().unwrap_or(&item.name);
                    self.define(local.name.clone(), TypeInfo::Unknown);
                }
            }
            Stmt::ExportDecl(inner) => self.check_stmt(inner)?,
            Stmt::Let(decl) | Stmt::Const(decl) | Stmt::Final(decl) => {
                let inferred = if let Some(initializer) = &decl.initializer {
                    let value_type = self.infer_expr(initializer)?;
                    if let Some(type_hint) = &decl.type_hint {
                        self.expect_assignable(
                            &value_type,
                            &type_from_ref(type_hint),
                            &format!("binding '{}'", decl.name.name),
                            decl.name.span,
                        )?;
                    }
                    value_type
                } else {
                    TypeInfo::Unknown
                };
                let binding_type = decl
                    .type_hint
                    .as_ref()
                    .map(type_from_ref)
                    .unwrap_or(inferred);
                self.define(decl.name.name.clone(), binding_type);
            }
            Stmt::Function(decl) => self.check_function(decl)?,
            Stmt::Class(decl) => self.check_class(decl)?,
            Stmt::TryCatch {
                try_block,
                catch_name,
                catch_block,
            } => {
                self.check_block(try_block)?;
                self.push_scope();
                self.define(catch_name.name.clone(), TypeInfo::known("String"));
                for stmt in catch_block {
                    self.check_stmt(stmt)?;
                }
                self.pop_scope();
            }
            Stmt::If {
                condition,
                then_branch,
                elifs,
                else_branch,
            } => {
                self.infer_expr(condition)?;
                self.check_block(then_branch)?;
                for (condition, body) in elifs {
                    self.infer_expr(condition)?;
                    self.check_block(body)?;
                }
                if let Some(body) = else_branch {
                    self.check_block(body)?;
                }
            }
            Stmt::While { condition, body } => {
                self.infer_expr(condition)?;
                self.check_block(body)?;
            }
            Stmt::Return { value, span } => {
                let value_type = if let Some(value) = value {
                    self.infer_expr(value)?
                } else {
                    TypeInfo::known("Nil")
                };
                if let Some(expected) = self.current_return.clone() {
                    self.expect_assignable(&value_type, &expected, "return value", *span)?;
                }
            }
            Stmt::Yield { value, .. } => {
                if let Some(value) = value {
                    self.infer_expr(value)?;
                }
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Expr(expr) => {
                self.infer_expr(expr)?;
            }
        }
        Ok(())
    }

    fn check_block(&mut self, statements: &[Stmt]) -> IcooResult<()> {
        self.push_scope();
        for stmt in statements {
            self.check_stmt(stmt)?;
        }
        self.pop_scope();
        Ok(())
    }

    fn check_function(&mut self, decl: &FunctionDecl) -> IcooResult<()> {
        self.push_scope();
        let previous_return = self.current_return.clone();
        self.current_return = decl.return_type.as_ref().map(type_from_ref);
        for param in &decl.params {
            let param_type = param
                .type_hint
                .as_ref()
                .map(type_from_ref)
                .unwrap_or(TypeInfo::Unknown);
            self.define(param.name.name.clone(), param_type);
        }
        for stmt in &decl.body {
            self.check_stmt(stmt)?;
        }
        self.current_return = previous_return;
        self.pop_scope();
        Ok(())
    }

    fn check_class(&mut self, decl: &ClassDecl) -> IcooResult<()> {
        let previous_class = self.current_class.clone();
        self.current_class = Some(decl.name.name.clone());
        for field in &decl.fields {
            if let Some(initializer) = &field.initializer {
                let value_type = self.infer_expr(initializer)?;
                self.expect_assignable(
                    &value_type,
                    &type_from_ref(&field.type_hint),
                    &format!("field '{}'", field.name.name),
                    field.name.span,
                )?;
            }
        }
        for method in &decl.methods {
            self.check_function(method)?;
        }
        self.current_class = previous_class;
        Ok(())
    }

    fn infer_expr(&mut self, expr: &Expr) -> IcooResult<TypeInfo> {
        match expr {
            Expr::Literal(literal, _) => Ok(match literal {
                Literal::Nil => TypeInfo::known("Nil"),
                Literal::Bool(_) => TypeInfo::known("Bool"),
                Literal::Int(_) => TypeInfo::known("Int"),
                Literal::Float(_) => TypeInfo::known("Float"),
                Literal::String(_) => TypeInfo::known("String"),
            }),
            Expr::Variable(name) => Ok(self.lookup(&name.name).unwrap_or(TypeInfo::Unknown)),
            Expr::Self_(_) => Ok(self
                .current_class
                .clone()
                .map(TypeInfo::known)
                .unwrap_or(TypeInfo::Unknown)),
            Expr::Super(_) => Ok(self
                .current_class
                .as_ref()
                .and_then(|name| self.classes.get(name))
                .and_then(|class| class.superclass.clone())
                .map(TypeInfo::known)
                .unwrap_or(TypeInfo::Unknown)),
            Expr::Array(values, _) => {
                let mut item_type = TypeInfo::Unknown;
                for value in values {
                    item_type = common_type(&item_type, &self.infer_expr(value)?);
                }
                Ok(TypeInfo::array(item_type))
            }
            Expr::Map(entries, _) => {
                let mut value_type = TypeInfo::Unknown;
                for (_, value) in entries {
                    value_type = common_type(&value_type, &self.infer_expr(value)?);
                }
                Ok(TypeInfo::map(TypeInfo::known("String"), value_type))
            }
            Expr::Template(parts, _) => {
                for part in parts {
                    if let TemplatePart::Expr(expr) = part {
                        self.infer_expr(expr)?;
                    }
                }
                Ok(TypeInfo::known("String"))
            }
            Expr::Unary { op, right, span } => {
                let right_type = self.infer_expr(right)?;
                match op {
                    UnaryOp::Not => Ok(TypeInfo::known("Bool")),
                    UnaryOp::Negate => {
                        self.expect_numeric(&right_type, *span)?;
                        Ok(right_type)
                    }
                }
            }
            Expr::Binary {
                left,
                op,
                right,
                span,
            } => {
                let left_type = self.infer_expr(left)?;
                let right_type = self.infer_expr(right)?;
                self.infer_binary(&left_type, *op, &right_type, *span)
            }
            Expr::Logical { left, right, .. } => {
                self.infer_expr(left)?;
                self.infer_expr(right)?;
                Ok(TypeInfo::known("Bool"))
            }
            Expr::Assign {
                target,
                value,
                span,
            } => {
                let value_type = self.infer_expr(value)?;
                match target.as_ref() {
                    Expr::Variable(name) => {
                        if let Some(expected) = self.lookup(&name.name) {
                            self.expect_assignable(
                                &value_type,
                                &expected,
                                &format!("binding '{}'", name.name),
                                *span,
                            )?;
                        }
                    }
                    Expr::Get { object, name, .. } => {
                        let object_type = self.infer_expr(object)?;
                        if let Some(field_type) = object_type
                            .name()
                            .and_then(|class_name| self.find_field_type(class_name, &name.name))
                        {
                            self.expect_assignable(
                                &value_type,
                                &field_type,
                                &format!("field '{}'", name.name),
                                *span,
                            )?;
                        }
                    }
                    _ => {}
                }
                Ok(value_type)
            }
            Expr::Get { object, name, .. } => {
                let object_type = self.infer_expr(object)?;
                Ok(object_type
                    .name()
                    .and_then(|type_name| self.find_property_type(type_name, &name.name))
                    .unwrap_or(TypeInfo::Unknown))
            }
            Expr::Call { callee, args, span } => self.infer_call(callee, args, *span),
            Expr::Await { task, .. } => {
                let task_type = self.infer_expr(task)?;
                match task_type {
                    TypeInfo::Task(result) => Ok(*result),
                    TypeInfo::Unknown => Ok(TypeInfo::Unknown),
                    other => {
                        self.expect_assignable(
                            &other,
                            &TypeInfo::known("Task"),
                            "await operand",
                            expr.span(),
                        )?;
                        Ok(TypeInfo::Unknown)
                    }
                }
            }
        }
    }

    fn infer_call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> IcooResult<TypeInfo> {
        match callee {
            Expr::Variable(name) => {
                if let Some(sig) = self.functions.get(&name.name).cloned() {
                    self.check_call_args(&sig, args, span)?;
                    return Ok(if sig.is_async {
                        TypeInfo::coroutine(sig.return_type.clone())
                    } else {
                        sig.return_type.clone()
                    });
                }
                if self.classes.contains_key(&name.name) {
                    if let Some(init) = self.find_method_sig(&name.name, "init") {
                        self.check_method_call_args(&init, args, span)?;
                    }
                    return Ok(TypeInfo::known(name.name.clone()));
                }
            }
            Expr::Get { object, name, .. } => {
                let object_type = self.infer_expr(object)?;
                if let Some(type_name) = object_type.name() {
                    if type_name == "EventLoop" {
                        if let Some(result) = self.infer_event_loop_call(&name.name, args, span)? {
                            return Ok(result);
                        }
                    }
                    if let Some(sig) = self.find_method_sig(type_name, &name.name) {
                        self.check_method_call_args(&sig, args, span)?;
                        return Ok(if sig.is_async {
                            TypeInfo::coroutine(sig.return_type.clone())
                        } else {
                            sig.return_type.clone()
                        });
                    }
                    if let Some(sig) = native_method_sig_for_receiver(&object_type, &name.name) {
                        self.check_native_method_call_args(&sig, args, span)?;
                        return Ok(sig.return_type);
                    }
                }
            }
            _ => {}
        }
        self.infer_expr(callee)?;
        for arg in args {
            self.infer_expr(arg)?;
        }
        Ok(TypeInfo::Unknown)
    }

    fn infer_event_loop_call(
        &mut self,
        method_name: &str,
        args: &[Expr],
        span: Span,
    ) -> IcooResult<Option<TypeInfo>> {
        match method_name {
            "spawn" if args.len() == 1 => {
                let coroutine_type = self.infer_expr(&args[0])?;
                match coroutine_type {
                    TypeInfo::Coroutine(result) => Ok(Some(TypeInfo::task(*result))),
                    TypeInfo::Unknown => Ok(Some(TypeInfo::task(TypeInfo::Unknown))),
                    other => {
                        self.expect_assignable(
                            &other,
                            &TypeInfo::known("Coroutine"),
                            "argument 1",
                            span,
                        )?;
                        Ok(Some(TypeInfo::task(TypeInfo::Unknown)))
                    }
                }
            }
            "run_until" if args.len() == 1 => {
                let task_type = self.infer_expr(&args[0])?;
                match task_type {
                    TypeInfo::Task(result) => Ok(Some(*result)),
                    TypeInfo::Unknown => Ok(Some(TypeInfo::Unknown)),
                    other => {
                        self.expect_assignable(
                            &other,
                            &TypeInfo::known("Task"),
                            "argument 1",
                            span,
                        )?;
                        Ok(Some(TypeInfo::Unknown))
                    }
                }
            }
            _ => Ok(None),
        }
    }

    fn infer_binary(
        &self,
        left: &TypeInfo,
        op: BinaryOp,
        right: &TypeInfo,
        span: Span,
    ) -> IcooResult<TypeInfo> {
        match op {
            BinaryOp::Add => match (left.name(), right.name()) {
                (Some("String"), _) | (_, Some("String")) => Ok(TypeInfo::known("String")),
                (Some("Int"), Some("Int")) => Ok(TypeInfo::known("Int")),
                (Some("Int" | "Float"), Some("Int" | "Float")) => Ok(TypeInfo::known("Float")),
                (Some(_), Some(_)) => {
                    Err(type_error("operator '+' expects numbers or strings", span))
                }
                _ => Ok(TypeInfo::Unknown),
            },
            BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide | BinaryOp::Remainder => {
                self.expect_numeric(left, span)?;
                self.expect_numeric(right, span)?;
                if left.name() == Some("Int")
                    && right.name() == Some("Int")
                    && !matches!(op, BinaryOp::Divide)
                {
                    Ok(TypeInfo::known("Int"))
                } else {
                    Ok(TypeInfo::known("Float"))
                }
            }
            BinaryOp::Equal
            | BinaryOp::NotEqual
            | BinaryOp::Less
            | BinaryOp::LessEqual
            | BinaryOp::Greater
            | BinaryOp::GreaterEqual => Ok(TypeInfo::known("Bool")),
        }
    }

    fn check_call_args(&mut self, sig: &FunctionSig, args: &[Expr], span: Span) -> IcooResult<()> {
        if sig.params.len() != args.len() {
            return Ok(());
        }
        for (index, arg) in args.iter().enumerate() {
            let arg_type = self.infer_expr(arg)?;
            if let Some(expected) = &sig.params[index] {
                self.expect_assignable(
                    &arg_type,
                    expected,
                    &format!("argument {}", index + 1),
                    span,
                )?;
            }
        }
        Ok(())
    }

    fn check_method_call_args(
        &mut self,
        sig: &FunctionSig,
        args: &[Expr],
        span: Span,
    ) -> IcooResult<()> {
        let params = if sig.params.first().is_some() {
            &sig.params[1..]
        } else {
            &sig.params[..]
        };
        if params.len() != args.len() {
            return Ok(());
        }
        for (index, arg) in args.iter().enumerate() {
            let arg_type = self.infer_expr(arg)?;
            if let Some(expected) = &params[index] {
                self.expect_assignable(
                    &arg_type,
                    expected,
                    &format!("argument {}", index + 1),
                    span,
                )?;
            }
        }
        Ok(())
    }

    fn check_native_method_call_args(
        &mut self,
        sig: &NativeMethodSig,
        args: &[Expr],
        span: Span,
    ) -> IcooResult<()> {
        if !sig.arity.accepts(args.len()) {
            return Err(type_error(
                format!(
                    "method expected {} arguments but got {}",
                    sig.arity.display(),
                    args.len()
                ),
                span,
            ));
        }
        for (index, arg) in args.iter().enumerate() {
            let arg_type = self.infer_expr(arg)?;
            let expected = sig
                .params
                .get(index)
                .and_then(|expected| expected.as_ref())
                .or(sig.variadic.as_ref());
            if let Some(expected) = expected {
                self.expect_assignable(
                    &arg_type,
                    expected,
                    &format!("argument {}", index + 1),
                    span,
                )?;
            }
        }
        Ok(())
    }

    fn expect_assignable(
        &self,
        actual: &TypeInfo,
        expected: &TypeInfo,
        context: &str,
        span: Span,
    ) -> IcooResult<()> {
        if self.is_assignable(actual, expected) {
            Ok(())
        } else {
            Err(type_error(
                format!(
                    "expected {} for {} but got {}",
                    expected.display_name(),
                    context,
                    actual.display_name()
                ),
                span,
            ))
        }
    }

    fn expect_numeric(&self, value: &TypeInfo, span: Span) -> IcooResult<()> {
        match value.name() {
            Some("Int" | "Float") | None => Ok(()),
            Some(_) => Err(type_error("expected numeric expression", span)),
        }
    }

    fn is_assignable(&self, actual: &TypeInfo, expected: &TypeInfo) -> bool {
        match (actual, expected) {
            (_, TypeInfo::Known(expected)) if expected == "Any" => return true,
            (TypeInfo::Unknown, _) => return true,
            (TypeInfo::Array(actual_item), TypeInfo::Array(expected_item)) => {
                return self.is_assignable(actual_item, expected_item);
            }
            (
                TypeInfo::Map(actual_key, actual_value),
                TypeInfo::Map(expected_key, expected_value),
            ) => {
                return self.is_assignable(actual_key, expected_key)
                    && self.is_assignable(actual_value, expected_value);
            }
            (TypeInfo::Task(actual_result), TypeInfo::Task(expected_result))
            | (TypeInfo::Coroutine(actual_result), TypeInfo::Coroutine(expected_result)) => {
                return self.is_assignable(actual_result, expected_result);
            }
            _ => {}
        }
        let Some(actual_name) = actual.name() else {
            return true;
        };
        let Some(expected_name) = expected.name() else {
            return true;
        };
        if expected_name == "Number" && matches!(actual_name, "Int" | "Float") {
            return true;
        }
        if actual_name == expected_name {
            return true;
        }
        let mut current = Some(actual_name.to_string());
        while let Some(class_name) = current {
            let Some(class) = self.classes.get(&class_name) else {
                break;
            };
            if class.superclass.as_deref() == Some(expected_name) {
                return true;
            }
            current = class.superclass.clone();
        }
        false
    }

    fn find_field_type(&self, class_name: &str, field_name: &str) -> Option<TypeInfo> {
        let mut current = Some(class_name.to_string());
        while let Some(name) = current {
            let class = self.classes.get(&name)?;
            if let Some(field_type) = class.fields.get(field_name) {
                return Some(field_type.clone());
            }
            current = class.superclass.clone();
        }
        None
    }

    fn find_property_type(&self, type_name: &str, property_name: &str) -> Option<TypeInfo> {
        if let Some(field_type) = self.find_field_type(type_name, property_name) {
            return Some(field_type);
        }
        if self.find_method_sig(type_name, property_name).is_some() {
            return Some(TypeInfo::known("Function"));
        }
        None
    }

    fn find_method_sig(&self, class_name: &str, method_name: &str) -> Option<FunctionSig> {
        let mut current = Some(class_name.to_string());
        while let Some(name) = current {
            let class = self.classes.get(&name)?;
            if let Some(sig) = class.methods.get(method_name) {
                return Some(sig.clone());
            }
            current = class.superclass.clone();
        }
        None
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn define(&mut self, name: String, ty: TypeInfo) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, ty);
        }
    }

    fn lookup(&self, name: &str) -> Option<TypeInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Some(value.clone());
            }
        }
        self.globals.get(name).cloned()
    }
}

impl NativeArity {
    fn accepts(self, actual: usize) -> bool {
        match self {
            NativeArity::Exact(expected) => actual == expected,
            NativeArity::Range { min, max } => (min..=max).contains(&actual),
            NativeArity::AtLeast(min) => actual >= min,
        }
    }

    fn display(self) -> String {
        match self {
            NativeArity::Exact(expected) => expected.to_string(),
            NativeArity::Range { min, max } => format!("{}..{}", min, max),
            NativeArity::AtLeast(min) => format!("at least {}", min),
        }
    }
}

fn function_sig(decl: &FunctionDecl) -> FunctionSig {
    FunctionSig {
        params: decl
            .params
            .iter()
            .map(|param| param.type_hint.as_ref().map(type_from_ref))
            .collect(),
        return_type: decl
            .return_type
            .as_ref()
            .map(type_from_ref)
            .unwrap_or(TypeInfo::Unknown),
        is_async: decl.is_coroutine,
    }
}

fn type_from_ref(type_ref: &TypeRef) -> TypeInfo {
    match type_ref.name.as_str() {
        "Array" => TypeInfo::array(
            type_ref
                .args
                .first()
                .map(type_from_ref)
                .unwrap_or(TypeInfo::Unknown),
        ),
        "Map" => TypeInfo::map(
            type_ref
                .args
                .first()
                .map(type_from_ref)
                .unwrap_or_else(|| TypeInfo::known("String")),
            type_ref
                .args
                .get(1)
                .map(type_from_ref)
                .unwrap_or(TypeInfo::Unknown),
        ),
        "Task" => TypeInfo::task(
            type_ref
                .args
                .first()
                .map(type_from_ref)
                .unwrap_or(TypeInfo::Unknown),
        ),
        "Coroutine" => TypeInfo::coroutine(
            type_ref
                .args
                .first()
                .map(type_from_ref)
                .unwrap_or(TypeInfo::Unknown),
        ),
        _ => TypeInfo::known(type_ref.name.clone()),
    }
}

fn common_type(left: &TypeInfo, right: &TypeInfo) -> TypeInfo {
    match (left, right) {
        (TypeInfo::Unknown, other) | (other, TypeInfo::Unknown) => other.clone(),
        (a, b) if a == b => a.clone(),
        _ => TypeInfo::known("Any"),
    }
}

fn import_module_type(source: &str) -> TypeInfo {
    native_modules::type_name(source)
        .map(TypeInfo::known)
        .unwrap_or_else(|| TypeInfo::known("Module"))
}

fn native_globals() -> HashMap<String, TypeInfo> {
    [
        ("print", "Function"),
        ("len", "Function"),
        ("str", "Function"),
        ("int", "Function"),
        ("float", "Function"),
        ("type", "Function"),
        ("EventLoop", "Function"),
        ("current_loop", "Function"),
        ("sleep", "Function"),
        ("math", "Math"),
        ("time", "Time"),
        ("json", "Json"),
        ("env", "Env"),
        ("Bytes", "BytesFactory"),
        ("Buffer", "BufferFactory"),
    ]
    .into_iter()
    .map(|(name, ty)| (name.to_string(), TypeInfo::known(ty)))
    .collect()
}

fn native_functions() -> HashMap<String, FunctionSig> {
    let mut functions = HashMap::new();
    functions.insert(
        "sleep".to_string(),
        FunctionSig {
            params: vec![Some(TypeInfo::known("Int"))],
            return_type: TypeInfo::task(TypeInfo::known("Nil")),
            is_async: false,
        },
    );
    functions.insert(
        "EventLoop".to_string(),
        FunctionSig {
            params: vec![Some(TypeInfo::known("Int"))],
            return_type: TypeInfo::known("EventLoop"),
            is_async: false,
        },
    );
    functions.insert(
        "current_loop".to_string(),
        FunctionSig {
            params: Vec::new(),
            return_type: TypeInfo::known("EventLoop"),
            is_async: false,
        },
    );
    functions
}

fn native_method_return(type_name: &str, method_name: &str) -> Option<TypeInfo> {
    match (type_name, method_name) {
        (_, "to_string") => Some(TypeInfo::known("String")),
        (_, "type_name") => Some(TypeInfo::known("String")),
        ("EventLoop", "spawn") => Some(TypeInfo::known("Task")),
        ("EventLoop", "run" | "stop") => Some(TypeInfo::known("Nil")),
        ("EventLoop", "run_until") => Some(TypeInfo::Unknown),
        ("EventLoop", "is_stopped") => Some(TypeInfo::known("Bool")),
        ("EventLoop", "backend_name") => Some(TypeInfo::known("String")),
        ("EventLoop", "worker_threads") => Some(TypeInfo::known("Int")),
        ("Task", "result") => Some(TypeInfo::Unknown),
        ("Task", "cancel") => Some(TypeInfo::known("Nil")),
        ("Task", "is_done" | "is_failed") => Some(TypeInfo::known("Bool")),
        ("Bool", "to_int") => Some(TypeInfo::known("Int")),
        ("Int", "to_float") => Some(TypeInfo::known("Float")),
        ("Int", "abs") => Some(TypeInfo::known("Int")),
        ("Float", "to_int") => Some(TypeInfo::known("Int")),
        ("Float", "abs") => Some(TypeInfo::known("Float")),
        ("Math", "floor" | "ceil" | "round") => Some(TypeInfo::known("Int")),
        ("Math", "random") => Some(TypeInfo::known("Float")),
        ("Math", "abs" | "min" | "max") => Some(TypeInfo::Unknown),
        ("Time", "now_ms" | "now_sec") => Some(TypeInfo::known("Int")),
        ("Json", "stringify") => Some(TypeInfo::known("String")),
        ("Json", "parse") => Some(TypeInfo::Unknown),
        ("Yaml", "stringify") => Some(TypeInfo::known("String")),
        ("Yaml", "parse") => Some(TypeInfo::Unknown),
        ("Toml", "stringify") => Some(TypeInfo::known("String")),
        ("Toml", "parse") => Some(TypeInfo::Unknown),
        ("Env", "cwd") => Some(TypeInfo::known("String")),
        ("Env", "args") => Some(TypeInfo::array(TypeInfo::known("String"))),
        ("Env", "get") => Some(TypeInfo::Unknown),
        ("Env", "has") => Some(TypeInfo::known("Bool")),
        ("Io", "print") => Some(TypeInfo::known("Nil")),
        ("IoFs", "exists" | "is_file" | "is_dir") => Some(TypeInfo::known("Bool")),
        ("IoFs", "read_text") => Some(TypeInfo::known("String")),
        ("IoFs", "read_bytes") => Some(TypeInfo::known("Bytes")),
        ("IoFs", "write_text" | "append_text" | "write_bytes" | "append_bytes") => {
            Some(TypeInfo::known("Nil"))
        }
        ("IoFs", "list_dir") => Some(TypeInfo::array(TypeInfo::known("String"))),
        ("Os", "name" | "family" | "arch" | "cwd") => Some(TypeInfo::known("String")),
        ("Os", "pid") => Some(TypeInfo::known("Int")),
        ("Os", "args") => Some(TypeInfo::array(TypeInfo::known("String"))),
        ("Os", "exe_path" | "get_env") => Some(TypeInfo::Unknown),
        ("Os", "has_env") => Some(TypeInfo::known("Bool")),
        (
            "NetHttpClient",
            "get" | "get_bytes" | "post" | "post_bytes" | "put" | "put_bytes" | "delete"
            | "delete_bytes" | "options" | "options_bytes" | "stream_get" | "stream_get_bytes"
            | "stream_post" | "stream_post_bytes" | "stream_put" | "stream_put_bytes"
            | "stream_delete" | "stream_options",
        ) => Some(TypeInfo::map(
            TypeInfo::known("String"),
            TypeInfo::known("Any"),
        )),
        ("NetHttpServer", "serve_once") => Some(TypeInfo::known("Nil")),
        ("WebIno", "App" | "create") => Some(TypeInfo::known("WebInoApp")),
        ("WebInoApp", "get" | "post" | "put" | "delete" | "options") => {
            Some(TypeInfo::known("WebInoApp"))
        }
        ("WebInoApp", "listen_once" | "listen" | "listen_with_workers") => {
            Some(TypeInfo::known("Nil"))
        }
        ("WebInoResponse", "status" | "header" | "content_type" | "write" | "write_bytes") => {
            Some(TypeInfo::known("WebInoResponse"))
        }
        ("WebInoResponse", "send" | "send_bytes" | "json" | "end" | "download") => {
            Some(TypeInfo::known("Nil"))
        }
        ("Array", "len" | "index_of" | "unshift" | "find_index") => Some(TypeInfo::known("Int")),
        ("Array", "is_empty" | "includes" | "some" | "every") => Some(TypeInfo::known("Bool")),
        ("Array", "push" | "for_each") => Some(TypeInfo::known("Nil")),
        ("Array", "pop" | "shift" | "reduce" | "find" | "at") => Some(TypeInfo::Unknown),
        ("Array", "join") => Some(TypeInfo::known("String")),
        ("Array", "slice" | "splice" | "reverse" | "map" | "filter") => {
            Some(TypeInfo::known("Array"))
        }
        ("Map", "len" | "size") => Some(TypeInfo::known("Int")),
        ("Map", "is_empty" | "has" | "delete") => Some(TypeInfo::known("Bool")),
        ("Map", "get") => Some(TypeInfo::Unknown),
        ("Map", "set") => Some(TypeInfo::known("Map")),
        ("Map", "clear" | "for_each") => Some(TypeInfo::known("Nil")),
        ("Map", "keys" | "values" | "entries") => Some(TypeInfo::known("Array")),
        ("String", "len") => Some(TypeInfo::known("Int")),
        ("String", "contains" | "is_empty") => Some(TypeInfo::known("Bool")),
        ("String", "to_bytes") => Some(TypeInfo::known("Bytes")),
        ("Bytes", "len") => Some(TypeInfo::known("Int")),
        ("Bytes", "is_empty" | "equals") => Some(TypeInfo::known("Bool")),
        ("Bytes", "slice" | "concat") => Some(TypeInfo::known("Bytes")),
        ("Bytes", "to_hex" | "to_base64") => Some(TypeInfo::known("String")),
        ("BytesFactory", "empty" | "from_hex" | "from_base64" | "from_string") => {
            Some(TypeInfo::known("Bytes"))
        }
        ("BufferFactory", "new" | "from_bytes" | "from_string") => Some(TypeInfo::known("Buffer")),
        ("Buffer", "len") => Some(TypeInfo::known("Int")),
        ("Buffer", "is_empty" | "equals") => Some(TypeInfo::known("Bool")),
        ("Buffer", "append" | "append_string") => Some(TypeInfo::known("Buffer")),
        ("Buffer", "slice" | "to_bytes") => Some(TypeInfo::known("Bytes")),
        ("Buffer", "clear") => Some(TypeInfo::known("Nil")),
        ("Buffer", "to_hex" | "to_base64") => Some(TypeInfo::known("String")),
        _ => None,
    }
}

fn native_method_return_for_receiver(receiver: &TypeInfo, method_name: &str) -> Option<TypeInfo> {
    match (receiver, method_name) {
        (TypeInfo::Coroutine(result), "to_string" | "type_name") => {
            let _ = result;
            Some(TypeInfo::known("String"))
        }
        (TypeInfo::Task(result), "result") => Some((**result).clone()),
        (TypeInfo::Task(_), "is_done" | "is_failed") => Some(TypeInfo::known("Bool")),
        (TypeInfo::Task(_), "to_string" | "type_name") => Some(TypeInfo::known("String")),
        _ => receiver
            .name()
            .and_then(|type_name| native_method_return(type_name, method_name)),
    }
}

fn native_method_sig_for_receiver(
    receiver: &TypeInfo,
    method_name: &str,
) -> Option<NativeMethodSig> {
    let return_type = native_method_return_for_receiver(receiver, method_name)?;
    if matches!(receiver.name(), Some("Bytes" | "Buffer")) && method_name == "to_string" {
        return Some(native_sig(
            NativeArity::Range { min: 0, max: 1 },
            vec![Some("String")],
            None,
            return_type,
        ));
    }
    if matches!(method_name, "to_string" | "type_name") {
        return Some(native_sig(NativeArity::Exact(0), vec![], None, return_type));
    }
    if let Some(sig) = receiver
        .name()
        .and_then(|type_name| native_modules::method_spec_for_type(type_name, method_name))
        .map(native_method_sig_from_spec)
    {
        return Some(sig);
    }
    match receiver {
        TypeInfo::Task(_) => match method_name {
            "result" | "is_done" | "is_failed" | "cancel" => {
                Some(native_sig(NativeArity::Exact(0), vec![], None, return_type))
            }
            _ => None,
        },
        TypeInfo::Coroutine(_) => None,
        _ => receiver
            .name()
            .and_then(|type_name| native_method_sig(type_name, method_name, return_type)),
    }
}

fn native_method_sig_from_spec(spec: &native_modules::NativeMethodSpec) -> NativeMethodSig {
    NativeMethodSig {
        arity: native_arity_from_spec(spec.arity),
        params: spec
            .params
            .iter()
            .map(|param| Some(native_type_from_spec(param)))
            .collect(),
        variadic: spec.variadic.map(native_type_from_spec),
        return_type: native_type_from_spec(spec.return_type),
    }
}

fn native_arity_from_spec(arity: native_modules::NativeAritySpec) -> NativeArity {
    match arity {
        native_modules::NativeAritySpec::Exact(expected) => NativeArity::Exact(expected),
        native_modules::NativeAritySpec::Range { min, max } => NativeArity::Range { min, max },
        native_modules::NativeAritySpec::AtLeast(min) => NativeArity::AtLeast(min),
    }
}

fn native_type_from_spec(name: &str) -> TypeInfo {
    match name {
        "Array<String>" => TypeInfo::array(TypeInfo::known("String")),
        "Map<String, Any>" => TypeInfo::map(TypeInfo::known("String"), TypeInfo::known("Any")),
        "Any" => TypeInfo::known("Any"),
        other => TypeInfo::known(other),
    }
}

fn native_method_sig(
    type_name: &str,
    method_name: &str,
    return_type: TypeInfo,
) -> Option<NativeMethodSig> {
    match (type_name, method_name) {
        ("String", "len" | "is_empty" | "to_bytes") => {
            Some(native_sig(NativeArity::Exact(0), vec![], None, return_type))
        }
        ("String", "contains") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("String")],
            None,
            return_type,
        )),
        ("Bool", "to_int")
        | ("Int", "to_float")
        | ("Int", "abs")
        | ("Float", "to_int")
        | ("Float", "abs")
        | ("Math", "random")
        | ("Time", "now_ms")
        | ("Time", "now_sec")
        | ("Env", "cwd")
        | ("Env", "args")
        | ("Os", "name")
        | ("Os", "family")
        | ("Os", "arch")
        | ("Os", "pid")
        | ("Os", "cwd")
        | ("Os", "args")
        | ("Os", "exe_path")
        | ("EventLoop", "run")
        | ("EventLoop", "stop")
        | ("EventLoop", "is_stopped")
        | ("EventLoop", "backend_name")
        | ("EventLoop", "worker_threads")
        | ("Array", "len")
        | ("Array", "is_empty")
        | ("Array", "pop")
        | ("Array", "shift")
        | ("Array", "reverse")
        | ("Map", "len")
        | ("Map", "size")
        | ("Map", "is_empty")
        | ("Map", "clear")
        | ("Map", "keys")
        | ("Map", "values")
        | ("Map", "entries") => Some(native_sig(NativeArity::Exact(0), vec![], None, return_type)),
        ("Math", "abs" | "floor" | "ceil" | "round") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("Number")],
            None,
            return_type,
        )),
        ("Math", "min" | "max") => Some(native_sig(
            NativeArity::Exact(2),
            vec![Some("Number"), Some("Number")],
            None,
            return_type,
        )),
        ("Json" | "Yaml" | "Toml", "stringify") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("Any")],
            None,
            return_type,
        )),
        ("Json" | "Yaml" | "Toml", "parse") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("String")],
            None,
            return_type,
        )),
        ("Env", "get" | "has") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("String")],
            None,
            return_type,
        )),
        ("Io", "print") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("Any")],
            None,
            return_type,
        )),
        ("IoFs", "exists" | "is_file" | "is_dir" | "read_text" | "read_bytes" | "list_dir") => {
            Some(native_sig(
                NativeArity::Exact(1),
                vec![Some("String")],
                None,
                return_type,
            ))
        }
        ("IoFs", "write_text" | "append_text") => Some(native_sig(
            NativeArity::Exact(2),
            vec![Some("String"), Some("String")],
            None,
            return_type,
        )),
        ("IoFs", "write_bytes" | "append_bytes") => Some(native_sig(
            NativeArity::Exact(2),
            vec![Some("String"), Some("Bytes")],
            None,
            return_type,
        )),
        ("Os", "get_env" | "has_env") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("String")],
            None,
            return_type,
        )),
        (
            "NetHttpClient",
            "get" | "get_bytes" | "delete" | "delete_bytes" | "options" | "options_bytes",
        ) => Some(native_sig(
            NativeArity::Range { min: 1, max: 2 },
            vec![Some("String"), Some("Map")],
            None,
            return_type,
        )),
        ("NetHttpClient", "post" | "put") => Some(native_sig(
            NativeArity::Range { min: 2, max: 3 },
            vec![Some("String"), Some("String"), Some("Map")],
            None,
            return_type,
        )),
        ("NetHttpClient", "post_bytes" | "put_bytes") => Some(native_sig(
            NativeArity::Range { min: 2, max: 3 },
            vec![Some("String"), Some("Bytes"), Some("Map")],
            None,
            return_type,
        )),
        ("NetHttpClient", "stream_get" | "stream_delete" | "stream_options") => Some(native_sig(
            NativeArity::Range { min: 2, max: 3 },
            vec![Some("String"), Some("Any"), Some("Function")],
            None,
            return_type,
        )),
        ("NetHttpClient", "stream_get_bytes") => Some(native_sig(
            NativeArity::Range { min: 2, max: 3 },
            vec![Some("String"), Some("Any"), Some("Function")],
            None,
            return_type,
        )),
        ("NetHttpClient", "stream_post" | "stream_put") => Some(native_sig(
            NativeArity::Range { min: 3, max: 4 },
            vec![
                Some("String"),
                Some("String"),
                Some("Any"),
                Some("Function"),
            ],
            None,
            return_type,
        )),
        ("NetHttpClient", "stream_post_bytes" | "stream_put_bytes") => Some(native_sig(
            NativeArity::Range { min: 3, max: 4 },
            vec![Some("String"), Some("Bytes"), Some("Any"), Some("Function")],
            None,
            return_type,
        )),
        ("NetHttpServer", "serve_once") => Some(native_sig(
            NativeArity::Exact(3),
            vec![Some("String"), Some("Int"), Some("String")],
            None,
            return_type,
        )),
        ("WebIno", "App" | "create") => {
            Some(native_sig(NativeArity::Exact(0), vec![], None, return_type))
        }
        ("WebInoApp", "get" | "post" | "put" | "delete" | "options") => Some(native_sig(
            NativeArity::Exact(2),
            vec![Some("String"), Some("Function")],
            None,
            return_type,
        )),
        ("WebInoApp", "listen_once") => Some(native_sig(
            NativeArity::Exact(2),
            vec![Some("String"), Some("Int")],
            None,
            return_type,
        )),
        ("WebInoApp", "listen") => Some(native_sig(
            NativeArity::Exact(3),
            vec![Some("String"), Some("Int"), Some("Int")],
            None,
            return_type,
        )),
        ("WebInoApp", "listen_with_workers") => Some(native_sig(
            NativeArity::Exact(4),
            vec![Some("String"), Some("Int"), Some("Int"), Some("Int")],
            None,
            return_type,
        )),
        ("WebInoResponse", "status") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("Int")],
            None,
            return_type,
        )),
        ("WebInoResponse", "header") => Some(native_sig(
            NativeArity::Exact(2),
            vec![Some("String"), Some("String")],
            None,
            return_type,
        )),
        ("WebInoResponse", "content_type") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("String")],
            None,
            return_type,
        )),
        ("WebInoResponse", "send" | "json" | "write") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("Any")],
            None,
            return_type,
        )),
        ("WebInoResponse", "send_bytes") => Some(native_sig(
            NativeArity::Range { min: 1, max: 2 },
            vec![Some("Bytes"), Some("String")],
            None,
            return_type,
        )),
        ("WebInoResponse", "write_bytes") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("Bytes")],
            None,
            return_type,
        )),
        ("WebInoResponse", "end") => {
            Some(native_sig(NativeArity::Exact(0), vec![], None, return_type))
        }
        ("WebInoResponse", "download") => Some(native_sig(
            NativeArity::Range { min: 1, max: 2 },
            vec![Some("String"), Some("String")],
            None,
            return_type,
        )),
        ("EventLoop", "spawn") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("Coroutine")],
            None,
            return_type,
        )),
        ("EventLoop", "run_until") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("Task")],
            None,
            return_type,
        )),
        ("Array", "push")
        | ("Array", "unshift")
        | ("Array", "includes")
        | ("Array", "index_of") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("Any")],
            None,
            return_type,
        )),
        ("Array", "at") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("Int")],
            None,
            return_type,
        )),
        ("Array", "slice") => Some(native_sig(
            NativeArity::Range { min: 1, max: 2 },
            vec![Some("Int"), Some("Int")],
            None,
            return_type,
        )),
        ("Array", "splice") => Some(native_sig(
            NativeArity::AtLeast(2),
            vec![Some("Int"), Some("Int")],
            Some("Any"),
            return_type,
        )),
        ("Array", "join") => Some(native_sig(
            NativeArity::Range { min: 0, max: 1 },
            vec![Some("String")],
            None,
            return_type,
        )),
        ("Array", "for_each")
        | ("Array", "map")
        | ("Array", "filter")
        | ("Array", "find")
        | ("Array", "find_index")
        | ("Array", "some")
        | ("Array", "every")
        | ("Map", "for_each") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("Function")],
            None,
            return_type,
        )),
        ("Array", "reduce") => Some(native_sig(
            NativeArity::Range { min: 1, max: 2 },
            vec![Some("Function"), Some("Any")],
            None,
            return_type,
        )),
        ("Map", "has" | "get" | "delete") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("String")],
            None,
            return_type,
        )),
        ("Map", "set") => Some(native_sig(
            NativeArity::Exact(2),
            vec![Some("String"), Some("Any")],
            None,
            return_type,
        )),
        ("Bytes", "len" | "is_empty" | "to_hex" | "to_base64") => {
            Some(native_sig(NativeArity::Exact(0), vec![], None, return_type))
        }
        ("Bytes", "to_string") => Some(native_sig(
            NativeArity::Range { min: 0, max: 1 },
            vec![Some("String")],
            None,
            return_type,
        )),
        ("Bytes", "slice") => Some(native_sig(
            NativeArity::Range { min: 1, max: 2 },
            vec![Some("Int"), Some("Int")],
            None,
            return_type,
        )),
        ("Bytes", "concat" | "equals") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("Bytes")],
            None,
            return_type,
        )),
        ("Buffer", "len" | "is_empty" | "to_bytes" | "clear" | "to_hex" | "to_base64") => {
            Some(native_sig(NativeArity::Exact(0), vec![], None, return_type))
        }
        ("Buffer", "append") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("Bytes")],
            None,
            return_type,
        )),
        ("Buffer", "append_string") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("String")],
            None,
            return_type,
        )),
        ("Buffer", "slice") => Some(native_sig(
            NativeArity::Range { min: 1, max: 2 },
            vec![Some("Int"), Some("Int")],
            None,
            return_type,
        )),
        ("Buffer", "equals") => Some(native_sig(
            NativeArity::Exact(1),
            vec![Some("Any")],
            None,
            return_type,
        )),
        _ => None,
    }
}

fn native_sig(
    arity: NativeArity,
    params: Vec<Option<&'static str>>,
    variadic: Option<&'static str>,
    return_type: TypeInfo,
) -> NativeMethodSig {
    NativeMethodSig {
        arity,
        params: params
            .into_iter()
            .map(|param| param.map(TypeInfo::known))
            .collect(),
        variadic: variadic.map(TypeInfo::known),
        return_type,
    }
}

fn type_error(message: impl Into<String>, span: Span) -> IcooError {
    IcooError::typecheck(message, span)
}
