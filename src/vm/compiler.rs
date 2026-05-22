use crate::error::{IcooError, IcooResult};
use crate::lexer;
use crate::parser::ast::{BindingDecl, Expr, Literal, LogicalOp, Program, Stmt, TemplatePart};
use crate::resolver;
use crate::runtime::env::BindingKind;
use crate::runtime::value::Value;
use crate::typechecker;
use crate::{parser, vm::instruction::Instruction};

#[derive(Debug, Clone)]
pub struct CompiledProgram {
    pub instructions: Vec<Instruction>,
}

pub fn compile_source(source: &str) -> IcooResult<CompiledProgram> {
    let tokens = lexer::lex(source)?;
    let program = parser::parse(tokens)?;
    resolver::resolve(&program)?;
    typechecker::check(&program)?;
    compile_program(&program)
}

pub fn compile_program(program: &Program) -> IcooResult<CompiledProgram> {
    let mut compiler = Compiler {
        instructions: Vec::new(),
    };
    compiler.compile_statements(&program.statements)?;
    Ok(CompiledProgram {
        instructions: compiler.instructions,
    })
}

struct Compiler {
    instructions: Vec<Instruction>,
}

impl Compiler {
    fn compile_statements(&mut self, statements: &[Stmt]) -> IcooResult<()> {
        for stmt in statements {
            self.compile_stmt(stmt)?;
        }
        Ok(())
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> IcooResult<()> {
        match stmt {
            Stmt::Let(decl) => self.compile_binding(decl, BindingKind::Mutable),
            Stmt::Const(decl) => self.compile_binding(decl, BindingKind::Const),
            Stmt::Final(decl) => self.compile_binding(decl, BindingKind::Final),
            Stmt::If {
                condition,
                then_branch,
                elifs,
                else_branch,
            } => self.compile_if(condition, then_branch, elifs, else_branch.as_deref()),
            Stmt::While { condition, body } => self.compile_while(condition, body),
            Stmt::Expr(expr) => {
                self.compile_expr(expr)?;
                self.instructions.push(Instruction::Pop);
                Ok(())
            }
            Stmt::ImportModule { span, .. } | Stmt::ImportNames { span, .. } => {
                Err(unsupported("imports", Some(*span)))
            }
            Stmt::ExportDecl(inner) => Err(unsupported("exports", Some(stmt_span(inner)))),
            Stmt::Function(decl) => Err(unsupported("function declarations", Some(decl.name.span))),
            Stmt::Class(decl) => Err(unsupported("classes", Some(decl.name.span))),
            Stmt::TryCatch { catch_name, .. } => {
                Err(unsupported("try/catch", Some(catch_name.span)))
            }
            Stmt::Return { span, .. } => Err(unsupported("return", Some(*span))),
            Stmt::Yield { span, .. } => Err(unsupported("yield", Some(*span))),
            Stmt::Break(span) => Err(unsupported("break", Some(*span))),
            Stmt::Continue(span) => Err(unsupported("continue", Some(*span))),
        }
    }

    fn compile_binding(&mut self, decl: &BindingDecl, kind: BindingKind) -> IcooResult<()> {
        let initialized = decl.initializer.is_some();
        if let Some(initializer) = &decl.initializer {
            self.compile_expr(initializer)?;
        } else {
            self.instructions.push(Instruction::Nil);
        }
        self.instructions.push(Instruction::Define {
            name: decl.name.name.clone(),
            kind,
            initialized,
        });
        Ok(())
    }

    fn compile_if(
        &mut self,
        condition: &Expr,
        then_branch: &[Stmt],
        elifs: &[(Expr, Vec<Stmt>)],
        else_branch: Option<&[Stmt]>,
    ) -> IcooResult<()> {
        let mut end_jumps = Vec::new();
        self.compile_expr(condition)?;
        let first_false_jump = self.push_jump_if_false_placeholder();
        self.instructions.push(Instruction::EnterScope);
        self.compile_statements(then_branch)?;
        self.instructions.push(Instruction::ExitScope);
        end_jumps.push(self.push_jump_placeholder());

        let mut previous_false_jump = first_false_jump;
        for (condition, body) in elifs {
            let start = self.instructions.len();
            self.patch_jump(previous_false_jump, start);
            self.compile_expr(condition)?;
            previous_false_jump = self.push_jump_if_false_placeholder();
            self.instructions.push(Instruction::EnterScope);
            self.compile_statements(body)?;
            self.instructions.push(Instruction::ExitScope);
            end_jumps.push(self.push_jump_placeholder());
        }

        let else_start = self.instructions.len();
        self.patch_jump(previous_false_jump, else_start);
        if let Some(else_branch) = else_branch {
            self.instructions.push(Instruction::EnterScope);
            self.compile_statements(else_branch)?;
            self.instructions.push(Instruction::ExitScope);
        }
        let end = self.instructions.len();
        for jump in end_jumps {
            self.patch_jump(jump, end);
        }
        Ok(())
    }

    fn compile_while(&mut self, condition: &Expr, body: &[Stmt]) -> IcooResult<()> {
        let loop_start = self.instructions.len();
        self.compile_expr(condition)?;
        let exit_jump = self.push_jump_if_false_placeholder();
        self.instructions.push(Instruction::EnterScope);
        self.compile_statements(body)?;
        self.instructions.push(Instruction::ExitScope);
        self.instructions.push(Instruction::Jump(loop_start));
        let end = self.instructions.len();
        self.patch_jump(exit_jump, end);
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expr) -> IcooResult<()> {
        match expr {
            Expr::Literal(literal, _) => {
                self.instructions
                    .push(Instruction::Constant(literal_value(literal)));
                Ok(())
            }
            Expr::Variable(name) => {
                self.instructions.push(Instruction::Load(name.name.clone()));
                Ok(())
            }
            Expr::Unary { op, right, .. } => {
                self.compile_expr(right)?;
                self.instructions.push(Instruction::Unary(*op));
                Ok(())
            }
            Expr::Binary {
                left, op, right, ..
            } => {
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                self.instructions.push(Instruction::Binary(*op));
                Ok(())
            }
            Expr::Logical {
                left, op, right, ..
            } => self.compile_logical(left, *op, right),
            Expr::Assign {
                target,
                value,
                span,
            } => {
                let Expr::Variable(name) = target.as_ref() else {
                    return Err(unsupported("non-variable assignment", Some(*span)));
                };
                self.compile_expr(value)?;
                self.instructions
                    .push(Instruction::Store(name.name.clone()));
                Ok(())
            }
            Expr::Call { callee, args, span } => self.compile_call(callee, args, *span),
            Expr::Get { object, name, span } if name.name == "to_string" => {
                self.compile_expr(object)?;
                self.instructions.push(Instruction::ToString);
                Ok(())
            }
            Expr::Template(parts, _) => self.compile_template(parts),
            Expr::Array(_, span) => Err(unsupported("arrays", Some(*span))),
            Expr::Map(_, span) => Err(unsupported("maps", Some(*span))),
            Expr::Get { span, .. } => Err(unsupported("property access", Some(*span))),
            Expr::Await { span, .. } => Err(unsupported("await", Some(*span))),
            Expr::Self_(span) => Err(unsupported("self", Some(*span))),
            Expr::Super(span) => Err(unsupported("super", Some(*span))),
        }
    }

    fn compile_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: crate::lexer::token::Span,
    ) -> IcooResult<()> {
        match callee {
            Expr::Variable(name) if name.name == "print" => {
                if args.len() != 1 {
                    return Err(IcooError::runtime(
                        format!(
                            "VM sync subset print expects 1 argument but got {}",
                            args.len()
                        ),
                        Some(span),
                    ));
                }
                self.compile_expr(&args[0])?;
                self.instructions.push(Instruction::Print);
                Ok(())
            }
            Expr::Get { object, name, .. } if name.name == "to_string" => {
                if !args.is_empty() {
                    return Err(IcooError::runtime(
                        "VM sync subset to_string expects 0 arguments",
                        Some(span),
                    ));
                }
                self.compile_expr(object)?;
                self.instructions.push(Instruction::ToString);
                Ok(())
            }
            _ => Err(unsupported("function calls", Some(span))),
        }
    }

    fn compile_logical(&mut self, left: &Expr, op: LogicalOp, right: &Expr) -> IcooResult<()> {
        self.compile_expr(left)?;
        let jump = match op {
            LogicalOp::And => {
                let index = self.instructions.len();
                self.instructions
                    .push(Instruction::JumpIfFalseKeep(usize::MAX));
                index
            }
            LogicalOp::Or => {
                let index = self.instructions.len();
                self.instructions
                    .push(Instruction::JumpIfTrueKeep(usize::MAX));
                index
            }
        };
        self.instructions.push(Instruction::Pop);
        self.compile_expr(right)?;
        let end = self.instructions.len();
        self.patch_jump(jump, end);
        Ok(())
    }

    fn compile_template(&mut self, parts: &[TemplatePart]) -> IcooResult<()> {
        let mut first = true;
        for part in parts {
            match part {
                TemplatePart::Text(text) => self
                    .instructions
                    .push(Instruction::Constant(Value::String(text.clone()))),
                TemplatePart::Expr(expr) => {
                    self.compile_expr(expr)?;
                    self.instructions.push(Instruction::ToString);
                }
            }
            if first {
                first = false;
            } else {
                self.instructions
                    .push(Instruction::Binary(crate::parser::ast::BinaryOp::Add));
            }
        }
        if first {
            self.instructions
                .push(Instruction::Constant(Value::String(String::new())));
        }
        Ok(())
    }

    fn push_jump_placeholder(&mut self) -> usize {
        let index = self.instructions.len();
        self.instructions.push(Instruction::Jump(usize::MAX));
        index
    }

    fn push_jump_if_false_placeholder(&mut self) -> usize {
        let index = self.instructions.len();
        self.instructions.push(Instruction::JumpIfFalse(usize::MAX));
        index
    }

    fn patch_jump(&mut self, index: usize, target: usize) {
        match &mut self.instructions[index] {
            Instruction::Jump(current)
            | Instruction::JumpIfFalse(current)
            | Instruction::JumpIfFalseKeep(current)
            | Instruction::JumpIfTrueKeep(current) => *current = target,
            _ => unreachable!("jump patch target is not a jump instruction"),
        }
    }
}

fn literal_value(literal: &Literal) -> Value {
    match literal {
        Literal::Nil => Value::Nil,
        Literal::Bool(value) => Value::Bool(*value),
        Literal::Int(value) => Value::Int(*value),
        Literal::Float(value) => Value::Float(*value),
        Literal::String(value) => Value::String(value.clone()),
    }
}

fn unsupported(feature: &str, span: Option<crate::lexer::token::Span>) -> IcooError {
    IcooError::runtime(format!("VM sync subset does not support {}", feature), span)
}

fn stmt_span(stmt: &Stmt) -> crate::lexer::token::Span {
    match stmt {
        Stmt::ImportModule { span, .. }
        | Stmt::ImportNames { span, .. }
        | Stmt::Break(span)
        | Stmt::Continue(span)
        | Stmt::Return { span, .. }
        | Stmt::Yield { span, .. } => *span,
        Stmt::Let(decl) | Stmt::Const(decl) | Stmt::Final(decl) => decl.name.span,
        Stmt::Function(decl) => decl.name.span,
        Stmt::Class(decl) => decl.name.span,
        Stmt::TryCatch { catch_name, .. } => catch_name.span,
        Stmt::If { condition, .. } | Stmt::While { condition, .. } | Stmt::Expr(condition) => {
            condition.span()
        }
        Stmt::ExportDecl(inner) => stmt_span(inner),
    }
}
