pub mod ast;

use crate::error::{IcooError, IcooResult};
use crate::lexer;
use crate::lexer::token::{Span, Token, TokenKind};
use ast::*;
use std::mem::discriminant;

pub fn parse(tokens: Vec<Token>) -> IcooResult<Program> {
    Parser::new(tokens).parse()
}

struct Parser {
    tokens: Vec<Token>,
    current: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, current: 0 }
    }

    fn parse(mut self) -> IcooResult<Program> {
        let mut statements = Vec::new();
        self.skip_newlines();
        while !self.is_at_end() {
            statements.push(self.declaration()?);
            self.skip_newlines();
        }
        Ok(Program { statements })
    }

    fn declaration(&mut self) -> IcooResult<Stmt> {
        if self.matches(&TokenKind::Import) {
            return self.import_module_stmt();
        }
        if self.matches(&TokenKind::From) {
            return self.import_names_stmt();
        }
        if self.matches(&TokenKind::Export) {
            return self.export_decl();
        }
        if self.matches(&TokenKind::Let) {
            return Ok(Stmt::Let(self.binding_decl(false, false)?));
        }
        if self.matches(&TokenKind::Const) {
            let decl = self.binding_decl(true, false)?;
            validate_const_name(&decl.name)?;
            return Ok(Stmt::Const(decl));
        }
        if self.matches(&TokenKind::Final) {
            return Ok(Stmt::Final(self.binding_decl(false, true)?));
        }
        if self.matches(&TokenKind::Async) {
            self.consume(TokenKind::Fn, "expected 'fn' after 'async'")?;
            return Ok(Stmt::Function(self.function_decl(true)?));
        }
        if self.matches(&TokenKind::Co) {
            self.consume(TokenKind::Fn, "expected 'fn' after 'co'")?;
            return Ok(Stmt::Function(self.function_decl(true)?));
        }
        if self.matches(&TokenKind::Fn) {
            return Ok(Stmt::Function(self.function_decl(false)?));
        }
        if self.matches(&TokenKind::Class) {
            return Ok(Stmt::Class(self.class_decl()?));
        }
        if self.matches(&TokenKind::Try) {
            return self.try_catch_stmt();
        }
        self.statement()
    }

    fn export_decl(&mut self) -> IcooResult<Stmt> {
        let span = self.previous().span;
        let stmt = if self.matches(&TokenKind::Let) {
            Stmt::Let(self.binding_decl(false, false)?)
        } else if self.matches(&TokenKind::Const) {
            let decl = self.binding_decl(true, false)?;
            validate_const_name(&decl.name)?;
            Stmt::Const(decl)
        } else if self.matches(&TokenKind::Final) {
            Stmt::Final(self.binding_decl(false, true)?)
        } else if self.matches(&TokenKind::Async) {
            self.consume(TokenKind::Fn, "expected 'fn' after 'async'")?;
            Stmt::Function(self.function_decl(true)?)
        } else if self.matches(&TokenKind::Co) {
            self.consume(TokenKind::Fn, "expected 'fn' after 'co'")?;
            Stmt::Function(self.function_decl(true)?)
        } else if self.matches(&TokenKind::Fn) {
            Stmt::Function(self.function_decl(false)?)
        } else if self.matches(&TokenKind::Class) {
            Stmt::Class(self.class_decl()?)
        } else {
            return Err(IcooError::parse(
                "expected declaration after 'export'",
                span,
            ));
        };
        Ok(Stmt::ExportDecl(Box::new(stmt)))
    }

    fn import_module_stmt(&mut self) -> IcooResult<Stmt> {
        let span = self.previous().span;
        let source = self.string_literal("expected module path after 'import'")?;
        self.consume(TokenKind::As, "expected 'as' after module path")?;
        let alias = self.identifier("expected import alias")?;
        self.consume_statement_end()?;
        Ok(Stmt::ImportModule {
            source,
            alias,
            span,
        })
    }

    fn import_names_stmt(&mut self) -> IcooResult<Stmt> {
        let span = self.previous().span;
        let source = self.string_literal("expected module path after 'from'")?;
        self.consume(TokenKind::Import, "expected 'import' after module path")?;
        let mut items = Vec::new();
        loop {
            let name = self.identifier("expected imported name")?;
            let alias = if self.matches(&TokenKind::As) {
                Some(self.identifier("expected import alias")?)
            } else {
                None
            };
            items.push(ImportItem { name, alias });
            if !self.matches(&TokenKind::Comma) {
                break;
            }
        }
        self.consume_statement_end()?;
        Ok(Stmt::ImportNames {
            source,
            items,
            span,
        })
    }

    fn binding_decl(
        &mut self,
        require_initializer: bool,
        final_decl: bool,
    ) -> IcooResult<BindingDecl> {
        let name = self.identifier("expected binding name")?;
        let type_hint = if self.matches(&TokenKind::Colon) {
            Some(self.type_ref()?)
        } else {
            None
        };
        let initializer = if self.matches(&TokenKind::Equal) {
            Some(self.expression()?)
        } else {
            None
        };
        if require_initializer && initializer.is_none() {
            return Err(self.error_here("const declarations must have an initializer"));
        }
        if final_decl && initializer.is_none() && type_hint.is_none() {
            return Err(IcooError::parse(
                "final declarations without an initializer must have a type annotation",
                name.span,
            ));
        }
        self.consume_statement_end()?;
        Ok(BindingDecl {
            name,
            type_hint,
            initializer,
        })
    }

    fn statement(&mut self) -> IcooResult<Stmt> {
        if self.matches(&TokenKind::If) {
            return self.if_stmt();
        }
        if self.matches(&TokenKind::While) {
            return self.while_stmt();
        }
        if self.matches(&TokenKind::Return) {
            let span = self.previous().span;
            let value = if self.is_statement_boundary() {
                None
            } else {
                Some(self.expression()?)
            };
            self.consume_statement_end()?;
            return Ok(Stmt::Return { value, span });
        }
        if self.matches(&TokenKind::Yield) {
            let span = self.previous().span;
            let value = if self.is_statement_boundary() {
                None
            } else {
                Some(self.expression()?)
            };
            self.consume_statement_end()?;
            return Ok(Stmt::Yield { value, span });
        }
        if self.matches(&TokenKind::Break) {
            let span = self.previous().span;
            self.consume_statement_end()?;
            return Ok(Stmt::Break(span));
        }
        if self.matches(&TokenKind::Continue) {
            let span = self.previous().span;
            self.consume_statement_end()?;
            return Ok(Stmt::Continue(span));
        }
        let expr = self.expression()?;
        self.consume_statement_end()?;
        Ok(Stmt::Expr(expr))
    }

    fn if_stmt(&mut self) -> IcooResult<Stmt> {
        let condition = self.expression()?;
        let then_branch = self.block()?;
        let mut elifs = Vec::new();
        let mut else_branch = None;
        self.skip_newlines();
        while self.matches(&TokenKind::Elif) {
            let cond = self.expression()?;
            let body = self.block()?;
            elifs.push((cond, body));
            self.skip_newlines();
        }
        if self.matches(&TokenKind::Else) {
            else_branch = Some(self.block()?);
        }
        Ok(Stmt::If {
            condition,
            then_branch,
            elifs,
            else_branch,
        })
    }

    fn while_stmt(&mut self) -> IcooResult<Stmt> {
        let condition = self.expression()?;
        let body = self.block()?;
        Ok(Stmt::While { condition, body })
    }

    fn try_catch_stmt(&mut self) -> IcooResult<Stmt> {
        let try_block = self.block()?;
        self.skip_newlines();
        self.consume(TokenKind::Catch, "expected 'catch' after try block")?;
        let catch_name = self.identifier("expected catch binding name")?;
        let catch_block = self.block()?;
        Ok(Stmt::TryCatch {
            try_block,
            catch_name,
            catch_block,
        })
    }

    fn function_decl(&mut self, is_coroutine: bool) -> IcooResult<FunctionDecl> {
        let name = self.identifier("expected function name")?;
        validate_method_name(&name)?;
        self.consume(TokenKind::LeftParen, "expected '(' after function name")?;
        let mut params = Vec::new();
        if !self.check(&TokenKind::RightParen) {
            loop {
                let name = self.identifier("expected parameter name")?;
                let type_hint = if self.matches(&TokenKind::Colon) {
                    Some(self.type_ref()?)
                } else {
                    None
                };
                params.push(Param { name, type_hint });
                if !self.matches(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.consume(TokenKind::RightParen, "expected ')' after parameters")?;
        let return_type = if self.matches(&TokenKind::Arrow) {
            Some(self.type_ref()?)
        } else {
            None
        };
        let body = self.block()?;
        Ok(FunctionDecl {
            name,
            params,
            return_type,
            body,
            is_coroutine,
        })
    }

    fn class_decl(&mut self) -> IcooResult<ClassDecl> {
        let name = self.identifier("expected class name")?;
        validate_class_name(&name)?;
        let superclass = if self.matches(&TokenKind::LeftArrow) {
            Some(self.identifier("expected superclass name after '<-'")?)
        } else {
            None
        };
        let (fields, methods) = self.class_body()?;
        Ok(ClassDecl {
            name,
            superclass,
            fields,
            methods,
        })
    }

    fn class_body(&mut self) -> IcooResult<(Vec<FieldDecl>, Vec<FunctionDecl>)> {
        self.consume(TokenKind::LeftBrace, "expected '{' after class declaration")?;

        let mut fields = Vec::new();
        let mut methods = Vec::new();
        self.skip_block_layout();
        while !self.check(&TokenKind::RightBrace) && !self.is_at_end() {
            if self.matches(&TokenKind::Async) {
                self.consume(TokenKind::Fn, "expected 'fn' after 'async'")?;
                methods.push(self.function_decl(true)?);
            } else if self.matches(&TokenKind::Co) {
                self.consume(TokenKind::Fn, "expected 'fn' after 'co'")?;
                methods.push(self.function_decl(true)?);
            } else if self.matches(&TokenKind::Fn) {
                methods.push(self.function_decl(false)?);
            } else if self.matches(&TokenKind::Let) {
                fields.push(self.field_decl(FieldKind::Mutable)?);
            } else if self.matches(&TokenKind::Const) {
                let field = self.field_decl(FieldKind::Const)?;
                validate_const_name(&field.name)?;
                if field.initializer.is_none() {
                    return Err(IcooError::parse(
                        "const fields must have an initializer",
                        field.name.span,
                    ));
                }
                fields.push(field);
            } else if self.matches(&TokenKind::Final) {
                fields.push(self.field_decl(FieldKind::Final)?);
            } else {
                return Err(self.error_here("expected field or method declaration in class body"));
            }
            self.skip_block_layout();
        }
        self.consume(TokenKind::RightBrace, "expected '}' after class body")?;
        Ok((fields, methods))
    }

    fn field_decl(&mut self, kind: FieldKind) -> IcooResult<FieldDecl> {
        let name = self.identifier("expected field name")?;
        self.consume(TokenKind::Colon, "expected ':' after field name")?;
        let type_hint = self.type_ref()?;
        let initializer = if self.matches(&TokenKind::Equal) {
            Some(self.expression()?)
        } else {
            None
        };
        self.consume_statement_end()?;
        Ok(FieldDecl {
            kind,
            name,
            type_hint,
            initializer,
        })
    }

    fn block(&mut self) -> IcooResult<Vec<Stmt>> {
        self.consume(TokenKind::LeftBrace, "expected '{' before block")?;
        let mut statements = Vec::new();
        self.skip_block_layout();
        while !self.check(&TokenKind::RightBrace) && !self.is_at_end() {
            statements.push(self.declaration()?);
            self.skip_block_layout();
        }
        self.consume(TokenKind::RightBrace, "expected '}' after block")?;
        Ok(statements)
    }

    fn expression(&mut self) -> IcooResult<Expr> {
        self.assignment()
    }

    fn assignment(&mut self) -> IcooResult<Expr> {
        let expr = self.ternary()?;
        if self.matches(&TokenKind::Equal) {
            let span = self.previous().span;
            let value = self.assignment()?;
            match expr {
                Expr::Variable(_) | Expr::Get { .. } => Ok(Expr::Assign {
                    target: Box::new(expr),
                    value: Box::new(value),
                    span,
                }),
                _ => Err(IcooError::parse("invalid assignment target", span)),
            }
        } else {
            Ok(expr)
        }
    }

    fn ternary(&mut self) -> IcooResult<Expr> {
        let expr = self.logic_or()?;
        if self.matches(&TokenKind::Question) {
            let span = self.previous().span;
            let then_expr = self.expression()?;
            self.consume(TokenKind::Colon, "expected ':' in ternary expression")?;
            let else_expr = self.ternary()?;
            Ok(Expr::Ternary {
                condition: Box::new(expr),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
                span,
            })
        } else {
            Ok(expr)
        }
    }

    fn logic_or(&mut self) -> IcooResult<Expr> {
        let mut expr = self.logic_and()?;
        while self.matches(&TokenKind::Or) {
            let span = self.previous().span;
            let right = self.logic_and()?;
            expr = Expr::Logical {
                left: Box::new(expr),
                op: LogicalOp::Or,
                right: Box::new(right),
                span,
            };
        }
        Ok(expr)
    }

    fn logic_and(&mut self) -> IcooResult<Expr> {
        let mut expr = self.equality()?;
        while self.matches(&TokenKind::And) {
            let span = self.previous().span;
            let right = self.equality()?;
            expr = Expr::Logical {
                left: Box::new(expr),
                op: LogicalOp::And,
                right: Box::new(right),
                span,
            };
        }
        Ok(expr)
    }

    fn equality(&mut self) -> IcooResult<Expr> {
        let mut expr = self.comparison()?;
        while self.matches_any(&[TokenKind::EqualEqual, TokenKind::BangEqual]) {
            let op = match self.previous().kind {
                TokenKind::EqualEqual => BinaryOp::Equal,
                _ => BinaryOp::NotEqual,
            };
            let span = self.previous().span;
            let right = self.comparison()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(expr)
    }

    fn comparison(&mut self) -> IcooResult<Expr> {
        let mut expr = self.term()?;
        while self.matches_any(&[
            TokenKind::Less,
            TokenKind::LessEqual,
            TokenKind::Greater,
            TokenKind::GreaterEqual,
        ]) {
            let op = match self.previous().kind {
                TokenKind::Less => BinaryOp::Less,
                TokenKind::LessEqual => BinaryOp::LessEqual,
                TokenKind::Greater => BinaryOp::Greater,
                _ => BinaryOp::GreaterEqual,
            };
            let span = self.previous().span;
            let right = self.term()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(expr)
    }

    fn term(&mut self) -> IcooResult<Expr> {
        let mut expr = self.factor()?;
        while self.matches_any(&[TokenKind::Plus, TokenKind::Minus]) {
            let op = match self.previous().kind {
                TokenKind::Plus => BinaryOp::Add,
                _ => BinaryOp::Subtract,
            };
            let span = self.previous().span;
            let right = self.factor()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(expr)
    }

    fn factor(&mut self) -> IcooResult<Expr> {
        let mut expr = self.unary()?;
        while self.matches_any(&[TokenKind::Star, TokenKind::Slash, TokenKind::Percent]) {
            let op = match self.previous().kind {
                TokenKind::Star => BinaryOp::Multiply,
                TokenKind::Slash => BinaryOp::Divide,
                _ => BinaryOp::Remainder,
            };
            let span = self.previous().span;
            let right = self.unary()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(expr)
    }

    fn unary(&mut self) -> IcooResult<Expr> {
        if self.matches(&TokenKind::Minus) {
            let span = self.previous().span;
            let right = self.unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Negate,
                right: Box::new(right),
                span,
            });
        }
        if self.matches(&TokenKind::Not) {
            let span = self.previous().span;
            let right = self.unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                right: Box::new(right),
                span,
            });
        }
        if self.matches(&TokenKind::Await) {
            let span = self.previous().span;
            let task = self.unary()?;
            return Ok(Expr::Await {
                task: Box::new(task),
                span,
            });
        }
        self.call()
    }

    fn call(&mut self) -> IcooResult<Expr> {
        let mut expr = self.primary()?;
        loop {
            if self.matches(&TokenKind::LeftParen) {
                let mut args = Vec::new();
                self.skip_newlines();
                if !self.check(&TokenKind::RightParen) {
                    loop {
                        args.push(self.expression()?);
                        self.skip_newlines();
                        if !self.matches(&TokenKind::Comma) {
                            break;
                        }
                        self.skip_newlines();
                        if self.check(&TokenKind::RightParen) {
                            break;
                        }
                    }
                }
                let paren = self.consume(TokenKind::RightParen, "expected ')' after arguments")?;
                expr = Expr::Call {
                    callee: Box::new(expr),
                    args,
                    span: paren.span,
                };
            } else if self.matches(&TokenKind::Dot) {
                let name = self.identifier("expected property name after '.'")?;
                let span = name.span;
                expr = Expr::Get {
                    object: Box::new(expr),
                    name,
                    span,
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn primary(&mut self) -> IcooResult<Expr> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Int(value) => Ok(Expr::Literal(Literal::Int(value), token.span)),
            TokenKind::Float(value) => Ok(Expr::Literal(Literal::Float(value), token.span)),
            TokenKind::String(value) => Ok(Expr::Literal(Literal::String(value), token.span)),
            TokenKind::TemplateString(value) => self.template_string(value, token.span),
            TokenKind::True => Ok(Expr::Literal(Literal::Bool(true), token.span)),
            TokenKind::False => Ok(Expr::Literal(Literal::Bool(false), token.span)),
            TokenKind::Nil => Ok(Expr::Literal(Literal::Nil, token.span)),
            TokenKind::Ident(name) => Ok(Expr::Variable(Identifier {
                name,
                span: token.span,
            })),
            TokenKind::Self_ => Ok(Expr::Self_(token.span)),
            TokenKind::Super => Ok(Expr::Super(token.span)),
            TokenKind::LeftParen => {
                let expr = self.expression()?;
                self.consume(TokenKind::RightParen, "expected ')' after expression")?;
                Ok(expr)
            }
            TokenKind::Match => self.match_expr(token.span),
            TokenKind::LeftBracket => self.array_literal(token.span),
            TokenKind::LeftBrace => self.map_literal(token.span),
            _ => Err(IcooError::parse("expected expression", token.span)),
        }
    }

    fn match_expr(&mut self, span: Span) -> IcooResult<Expr> {
        let value = self.expression()?;
        self.consume(TokenKind::LeftBrace, "expected '{' after match value")?;
        let mut arms = Vec::new();
        self.skip_newlines();
        while !self.check(&TokenKind::RightBrace) && !self.is_at_end() {
            let pattern = if self.matches_ident("_") {
                MatchPattern::Wildcard(self.previous().span)
            } else {
                MatchPattern::Expr(self.expression()?)
            };
            self.consume(TokenKind::FatArrow, "expected '=>' after match pattern")?;
            self.skip_newlines();
            let arm_value = self.expression()?;
            arms.push(MatchArm {
                pattern,
                value: arm_value,
            });
            self.skip_newlines();
            if !self.matches(&TokenKind::Comma) {
                break;
            }
            self.skip_newlines();
        }
        self.consume(TokenKind::RightBrace, "expected '}' after match arms")?;
        Ok(Expr::Match {
            value: Box::new(value),
            arms,
            span,
        })
    }

    fn array_literal(&mut self, span: Span) -> IcooResult<Expr> {
        let mut values = Vec::new();
        self.skip_newlines();
        if !self.check(&TokenKind::RightBracket) {
            loop {
                values.push(self.expression()?);
                self.skip_newlines();
                if !self.matches(&TokenKind::Comma) {
                    break;
                }
                self.skip_newlines();
                if self.check(&TokenKind::RightBracket) {
                    break;
                }
            }
        }
        self.consume(TokenKind::RightBracket, "expected ']' after array literal")?;
        Ok(Expr::Array(values, span))
    }

    fn map_literal(&mut self, span: Span) -> IcooResult<Expr> {
        let mut entries = Vec::new();
        self.skip_newlines();
        if !self.check(&TokenKind::RightBrace) {
            loop {
                let key = match self.advance().kind.clone() {
                    TokenKind::String(value) => value,
                    _ => return Err(self.error_previous("expected string key in map literal")),
                };
                self.consume(TokenKind::Colon, "expected ':' after map key")?;
                self.skip_newlines();
                let value = self.expression()?;
                entries.push((key, value));
                self.skip_newlines();
                if !self.matches(&TokenKind::Comma) {
                    break;
                }
                self.skip_newlines();
                if self.check(&TokenKind::RightBrace) {
                    break;
                }
            }
        }
        self.skip_newlines();
        self.consume(TokenKind::RightBrace, "expected '}' after map literal")?;
        Ok(Expr::Map(entries, span))
    }

    fn type_ref(&mut self) -> IcooResult<TypeRef> {
        let ident = self.identifier("expected type name")?;
        let mut args = Vec::new();
        if self.matches(&TokenKind::Less) {
            loop {
                args.push(self.type_ref()?);
                if !self.matches(&TokenKind::Comma) {
                    break;
                }
            }
            self.consume(
                TokenKind::Greater,
                "expected '>' after generic type arguments",
            )?;
        }
        Ok(TypeRef {
            name: ident.name,
            args,
            span: ident.span,
        })
    }

    fn template_string(&self, value: String, span: Span) -> IcooResult<Expr> {
        let mut parts = Vec::new();
        let chars: Vec<char> = value.chars().collect();
        let mut text = String::new();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '{' if chars.get(i + 1) == Some(&'{') => {
                    text.push('{');
                    i += 2;
                }
                '}' if chars.get(i + 1) == Some(&'}') => {
                    text.push('}');
                    i += 2;
                }
                '{' => {
                    if !text.is_empty() {
                        parts.push(TemplatePart::Text(std::mem::take(&mut text)));
                    }
                    let (expr_source, next) = read_template_expr(&chars, i + 1, span)?;
                    let expr = parse_template_expr(&expr_source, span)?;
                    parts.push(TemplatePart::Expr(expr));
                    i = next;
                }
                '}' => {
                    return Err(IcooError::parse(
                        "unexpected '}' in template string; use '}}' for a literal brace",
                        span,
                    ));
                }
                ch => {
                    text.push(ch);
                    i += 1;
                }
            }
        }
        if !text.is_empty() {
            parts.push(TemplatePart::Text(text));
        }
        Ok(Expr::Template(parts, span))
    }

    fn identifier(&mut self, message: &str) -> IcooResult<Identifier> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Ident(name) => Ok(Identifier {
                name,
                span: token.span,
            }),
            TokenKind::Self_ => Ok(Identifier {
                name: "self".to_string(),
                span: token.span,
            }),
            _ => Err(IcooError::parse(message, token.span)),
        }
    }

    fn string_literal(&mut self, message: &str) -> IcooResult<String> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::String(value) => Ok(value),
            _ => Err(IcooError::parse(message, token.span)),
        }
    }

    fn consume_statement_end(&mut self) -> IcooResult<()> {
        if self.matches(&TokenKind::Newline)
            || self.check(&TokenKind::RightBrace)
            || self.check(&TokenKind::Eof)
        {
            Ok(())
        } else {
            Err(self.error_here("expected end of statement"))
        }
    }

    fn consume(&mut self, kind: TokenKind, message: &str) -> IcooResult<Token> {
        if self.check(&kind) {
            Ok(self.advance().clone())
        } else {
            Err(self.error_here(message))
        }
    }

    fn matches(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn matches_any(&mut self, kinds: &[TokenKind]) -> bool {
        for kind in kinds {
            if self.check(kind) {
                self.advance();
                return true;
            }
        }
        false
    }

    fn matches_ident(&mut self, expected: &str) -> bool {
        match &self.peek().kind {
            TokenKind::Ident(name) if name == expected => {
                self.advance();
                true
            }
            _ => false,
        }
    }

    fn check(&self, kind: &TokenKind) -> bool {
        if self.is_at_end() && !same_variant(&self.peek().kind, kind) {
            return false;
        }
        same_variant(&self.peek().kind, kind)
    }

    fn advance(&mut self) -> &Token {
        if !self.is_at_end() {
            self.current += 1;
        }
        self.previous()
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }

    fn previous(&self) -> &Token {
        &self.tokens[self.current - 1]
    }

    fn skip_newlines(&mut self) {
        while self.check(&TokenKind::Newline) {
            self.advance();
        }
    }

    fn skip_block_layout(&mut self) {
        while self.check(&TokenKind::Newline) {
            self.advance();
        }
    }

    fn is_statement_boundary(&self) -> bool {
        self.check(&TokenKind::Newline)
            || self.check(&TokenKind::RightBrace)
            || self.check(&TokenKind::Eof)
    }

    fn error_here(&self, message: &str) -> IcooError {
        IcooError::parse(message, self.peek().span)
    }

    fn error_previous(&self, message: &str) -> IcooError {
        IcooError::parse(message, self.previous().span)
    }
}

fn same_variant(a: &TokenKind, b: &TokenKind) -> bool {
    discriminant(a) == discriminant(b)
}

fn validate_const_name(name: &Identifier) -> IcooResult<()> {
    let ok = name
        .name
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && name.name.chars().any(|c| c.is_ascii_uppercase())
        && !name.name.chars().next().is_some_and(|c| c.is_ascii_digit());
    if ok {
        Ok(())
    } else {
        Err(IcooError::parse(
            format!(
                "constant name '{}' must use uppercase letters, digits, or '_'",
                name.name
            ),
            name.span,
        ))
    }
}

fn validate_class_name(name: &Identifier) -> IcooResult<()> {
    if name
        .name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(IcooError::parse(
            format!(
                "class name '{}' must start with an uppercase letter",
                name.name
            ),
            name.span,
        ))
    }
}

fn validate_method_name(name: &Identifier) -> IcooResult<()> {
    let ok = name
        .name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && name
            .name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(IcooError::parse(
            format!("method name '{}' must use snake_case", name.name),
            name.span,
        ))
    }
}

fn read_template_expr(chars: &[char], start: usize, span: Span) -> IcooResult<(String, usize)> {
    let mut depth = 0usize;
    let mut expr = String::new();
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '{' => {
                depth += 1;
                expr.push('{');
            }
            '}' if depth == 0 => {
                if expr.trim().is_empty() {
                    return Err(IcooError::parse("empty template expression", span));
                }
                return Ok((expr, i + 1));
            }
            '}' => {
                depth -= 1;
                expr.push('}');
            }
            ch => expr.push(ch),
        }
        i += 1;
    }
    Err(IcooError::parse("unterminated template expression", span))
}

fn parse_template_expr(source: &str, span: Span) -> IcooResult<Expr> {
    let tokens = lexer::lex(source)?;
    let mut parser = Parser::new(tokens);
    let expr = parser.expression()?;
    if !parser.check(&TokenKind::Newline) && !parser.check(&TokenKind::Eof) {
        return Err(IcooError::parse("invalid template expression", span));
    }
    Ok(expr)
}
