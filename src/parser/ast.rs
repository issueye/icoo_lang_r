use crate::lexer::token::Span;

#[derive(Debug, Clone)]
pub struct Program {
    pub statements: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    ImportModule {
        source: String,
        alias: Identifier,
        span: Span,
    },
    ImportNames {
        source: String,
        items: Vec<ImportItem>,
        span: Span,
    },
    ExportDecl(Box<Stmt>),
    Let(BindingDecl),
    Const(BindingDecl),
    Final(BindingDecl),
    Function(FunctionDecl),
    Class(ClassDecl),
    If {
        condition: Expr,
        then_branch: Vec<Stmt>,
        elifs: Vec<(Expr, Vec<Stmt>)>,
        else_branch: Option<Vec<Stmt>>,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    Yield {
        value: Option<Expr>,
        span: Span,
    },
    Break(Span),
    Continue(Span),
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub struct ImportItem {
    pub name: Identifier,
    pub alias: Option<Identifier>,
}

#[derive(Debug, Clone)]
pub struct BindingDecl {
    pub name: Identifier,
    pub type_hint: Option<TypeRef>,
    pub initializer: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct FunctionDecl {
    pub name: Identifier,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    pub body: Vec<Stmt>,
    pub is_coroutine: bool,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: Identifier,
    pub type_hint: Option<TypeRef>,
}

#[derive(Debug, Clone)]
pub struct ClassDecl {
    pub name: Identifier,
    pub superclass: Option<Identifier>,
    pub fields: Vec<FieldDecl>,
    pub methods: Vec<FunctionDecl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Mutable,
    Const,
    Final,
}

#[derive(Debug, Clone)]
pub struct FieldDecl {
    pub kind: FieldKind,
    pub name: Identifier,
    pub type_hint: TypeRef,
    pub initializer: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct Identifier {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TypeRef {
    pub name: String,
    pub args: Vec<TypeRef>,
    pub span: Span,
}

impl TypeRef {
    pub fn simple(name: impl Into<String>, span: Span) -> Self {
        Self {
            name: name.into(),
            args: Vec::new(),
            span,
        }
    }

    pub fn display_name(&self) -> String {
        if self.args.is_empty() {
            self.name.clone()
        } else {
            let args = self
                .args
                .iter()
                .map(TypeRef::display_name)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}<{}>", self.name, args)
        }
    }
}

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(Literal, Span),
    Variable(Identifier),
    Self_(Span),
    Super(Span),
    Array(Vec<Expr>, Span),
    Map(Vec<(String, Expr)>, Span),
    Template(Vec<TemplatePart>, Span),
    Unary {
        op: UnaryOp,
        right: Box<Expr>,
        span: Span,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
        span: Span,
    },
    Logical {
        left: Box<Expr>,
        op: LogicalOp,
        right: Box<Expr>,
        span: Span,
    },
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
        span: Span,
    },
    Get {
        object: Box<Expr>,
        name: Identifier,
        span: Span,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    Await {
        task: Box<Expr>,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal(_, span)
            | Expr::Self_(span)
            | Expr::Super(span)
            | Expr::Array(_, span)
            | Expr::Map(_, span)
            | Expr::Template(_, span)
            | Expr::Unary { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Logical { span, .. }
            | Expr::Assign { span, .. }
            | Expr::Get { span, .. }
            | Expr::Call { span, .. }
            | Expr::Await { span, .. } => *span,
            Expr::Variable(ident) => ident.span,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Literal {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
}

#[derive(Debug, Clone)]
pub enum TemplatePart {
    Text(String),
    Expr(Expr),
}

#[derive(Debug, Clone, Copy)]
pub enum UnaryOp {
    Negate,
    Not,
}

#[derive(Debug, Clone, Copy)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Debug, Clone, Copy)]
pub enum LogicalOp {
    And,
    Or,
}
