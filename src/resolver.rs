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
        self.resolve_statements(&program.statements)
    }

    fn resolve_statements(&mut self, statements: &[Stmt]) -> IcooResult<()> {
        for stmt in statements {
            self.resolve_stmt(stmt)?;
        }
        Ok(())
    }

    fn resolve_stmt(&mut self, stmt: &Stmt) -> IcooResult<()> {
        match stmt {
            Stmt::Let(decl) | Stmt::Const(decl) | Stmt::Final(decl) => {
                if let Some(initializer) = &decl.initializer {
                    self.resolve_expr(initializer)?;
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
            Stmt::Return { value, span } => {
                if self.function == FunctionContext::None {
                    return Err(IcooError::resolve(
                        "return can only be used inside a function",
                        *span,
                    ));
                }
                if let Some(value) = value {
                    self.resolve_expr(value)?;
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
            Stmt::Expr(expr) => self.resolve_expr(expr)?,
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
                self.resolve_expr(task)?;
            }
        }
        Ok(())
    }
}
