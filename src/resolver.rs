use crate::error::{IcooError, IcooResult};
use crate::parser::ast::*;

pub fn resolve(program: &Program) -> IcooResult<()> {
    Resolver::new().resolve_program(program)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FunctionContext {
    None,
    Sync,
    Async,
}

struct Resolver {
    function: FunctionContext,
    loop_depth: usize,
}

impl Resolver {
    fn new() -> Self {
        Self {
            function: FunctionContext::None,
            loop_depth: 0,
        }
    }

    fn resolve_program(&mut self, program: &Program) -> IcooResult<()> {
        for stmt in &program.statements {
            self.resolve_stmt(stmt, true)?;
        }
        Ok(())
    }

    fn resolve_statements(&mut self, statements: &[Stmt]) -> IcooResult<()> {
        for stmt in statements {
            self.resolve_stmt(stmt, false)?;
        }
        Ok(())
    }

    fn resolve_stmt(&mut self, stmt: &Stmt, top_level: bool) -> IcooResult<()> {
        match stmt {
            Stmt::ImportModule { span, .. } | Stmt::ImportNames { span, .. } => {
                if !top_level {
                    return Err(IcooError::resolve(
                        "import can only be used at module top level",
                        *span,
                    ));
                }
            }
            Stmt::ExportDecl(inner) => {
                if !top_level {
                    return Err(IcooError::resolve(
                        "export can only be used at module top level",
                        stmt_span(inner),
                    ));
                }
                self.resolve_stmt(inner, true)?;
            }
            Stmt::Let(decl) | Stmt::Const(decl) | Stmt::Final(decl) => {
                if let Some(initializer) = &decl.initializer {
                    self.resolve_expr_with_await_policy(
                        initializer,
                        matches!(initializer, Expr::Await { .. }),
                    )?;
                }
            }
            Stmt::Function(decl) => self.resolve_function(decl)?,
            Stmt::Class(decl) => {
                for field in &decl.fields {
                    if let Some(initializer) = &field.initializer {
                        self.resolve_expr(initializer)?;
                    }
                }
                for method in &decl.methods {
                    self.resolve_function(method)?;
                }
            }
            Stmt::TryCatch {
                try_block,
                catch_block,
                ..
            } => {
                self.resolve_statements(try_block)?;
                self.resolve_statements(catch_block)?;
            }
            Stmt::If {
                condition,
                then_branch,
                elifs,
                else_branch,
            } => {
                self.resolve_expr(condition)?;
                self.resolve_statements(then_branch)?;
                for (condition, body) in elifs {
                    self.resolve_expr(condition)?;
                    self.resolve_statements(body)?;
                }
                if let Some(body) = else_branch {
                    self.resolve_statements(body)?;
                }
            }
            Stmt::While { condition, body } => {
                self.resolve_expr(condition)?;
                self.loop_depth += 1;
                let result = self.resolve_statements(body);
                self.loop_depth -= 1;
                result?;
            }
            Stmt::Match { value, arms, .. } => {
                self.resolve_expr(value)?;
                for arm in arms {
                    if let MatchPattern::Expr(pattern) = &arm.pattern {
                        self.resolve_expr(pattern)?;
                    }
                    self.resolve_stmt(&arm.body, false)?;
                }
            }
            Stmt::Return { value, span } => {
                if self.function == FunctionContext::None {
                    return Err(IcooError::resolve(
                        "return can only be used inside a function",
                        *span,
                    ));
                }
                if let Some(value) = value {
                    self.resolve_expr_with_await_policy(
                        value,
                        matches!(value, Expr::Await { .. }),
                    )?;
                }
            }
            Stmt::Yield { value, span } => {
                if self.function != FunctionContext::Async {
                    return Err(IcooError::resolve(
                        "yield can only be used inside an async fn",
                        *span,
                    ));
                }
                if let Some(value) = value {
                    self.resolve_expr(value)?;
                }
            }
            Stmt::Break(span) => {
                if self.loop_depth == 0 {
                    return Err(IcooError::resolve(
                        "break can only be used inside a loop",
                        *span,
                    ));
                }
            }
            Stmt::Continue(span) => {
                if self.loop_depth == 0 {
                    return Err(IcooError::resolve(
                        "continue can only be used inside a loop",
                        *span,
                    ));
                }
            }
            Stmt::Expr(expr) => {
                self.resolve_expr_with_await_policy(expr, matches!(expr, Expr::Await { .. }))?
            }
        }
        Ok(())
    }

    fn resolve_function(&mut self, decl: &FunctionDecl) -> IcooResult<()> {
        let previous_function = self.function;
        let previous_loop_depth = self.loop_depth;
        self.function = if decl.is_coroutine {
            FunctionContext::Async
        } else {
            FunctionContext::Sync
        };
        self.loop_depth = 0;
        let result = self.resolve_statements(&decl.body);
        self.function = previous_function;
        self.loop_depth = previous_loop_depth;
        result
    }

    fn resolve_expr(&mut self, expr: &Expr) -> IcooResult<()> {
        self.resolve_expr_with_await_policy(expr, false)
    }

    fn resolve_expr_with_await_policy(
        &mut self,
        expr: &Expr,
        allow_direct_await: bool,
    ) -> IcooResult<()> {
        match expr {
            Expr::Literal(_, _) | Expr::Variable(_) | Expr::Self_(_) | Expr::Super(_) => {}
            Expr::Array(values, _) => {
                for value in values {
                    self.resolve_expr(value)?;
                }
            }
            Expr::Map(entries, _) => {
                for (_, value) in entries {
                    self.resolve_expr(value)?;
                }
            }
            Expr::Template(parts, _) => {
                for part in parts {
                    if let TemplatePart::Expr(expr) = part {
                        self.resolve_expr(expr)?;
                    }
                }
            }
            Expr::Unary { right, .. } => self.resolve_expr(right)?,
            Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
                self.resolve_expr(left)?;
                self.resolve_expr(right)?;
            }
            Expr::Ternary {
                condition,
                then_expr,
                else_expr,
                ..
            } => {
                self.resolve_expr(condition)?;
                self.resolve_expr(then_expr)?;
                self.resolve_expr(else_expr)?;
            }
            Expr::Match { value, arms, .. } => {
                self.resolve_expr(value)?;
                for arm in arms {
                    if let MatchPattern::Expr(pattern) = &arm.pattern {
                        self.resolve_expr(pattern)?;
                    }
                    self.resolve_expr(&arm.value)?;
                }
            }
            Expr::Assign { target, value, .. } => {
                self.resolve_expr(target)?;
                self.resolve_expr(value)?;
            }
            Expr::Get { object, .. } => self.resolve_expr(object)?,
            Expr::Call { callee, args, .. } => {
                self.resolve_expr(callee)?;
                for arg in args {
                    self.resolve_expr(arg)?;
                }
            }
            Expr::Await { task, span } => {
                if self.function != FunctionContext::Async {
                    return Err(IcooError::resolve(
                        "await can only be used inside an async fn",
                        *span,
                    ));
                }
                if !allow_direct_await {
                    return Err(IcooError::resolve(
                        "await can only be used as a standalone expression, binding initializer, or return value",
                        *span,
                    ));
                }
                self.resolve_expr(task)?;
            }
        }
        Ok(())
    }
}

fn stmt_span(stmt: &Stmt) -> crate::lexer::token::Span {
    match stmt {
        Stmt::ImportModule { span, .. }
        | Stmt::ImportNames { span, .. }
        | Stmt::Return { span, .. }
        | Stmt::Yield { span, .. }
        | Stmt::Break(span)
        | Stmt::Continue(span) => *span,
        Stmt::ExportDecl(inner) => stmt_span(inner),
        Stmt::Let(decl) | Stmt::Const(decl) | Stmt::Final(decl) => decl.name.span,
        Stmt::Function(decl) => decl.name.span,
        Stmt::Class(decl) => decl.name.span,
        Stmt::TryCatch { catch_name, .. } => catch_name.span,
        Stmt::If { condition, .. } | Stmt::While { condition, .. } | Stmt::Expr(condition) => {
            condition.span()
        }
        Stmt::Match { span, .. } => *span,
    }
}
