#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub column: usize,
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(line: usize, column: usize, start: usize, end: usize) -> Self {
        Self {
            line,
            column,
            start,
            end,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Int(i64),
    Float(f64),
    String(String),
    TemplateString(String),

    Let,
    Const,
    Final,
    Async,
    Co,
    Fn,
    Class,
    If,
    Elif,
    Else,
    While,
    Return,
    Break,
    Continue,
    Yield,
    Await,
    True,
    False,
    Nil,
    Self_,
    Super,
    And,
    Or,
    Not,

    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    Comma,
    Dot,
    Colon,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Equal,
    EqualEqual,
    BangEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Arrow,
    LeftArrow,

    Newline,
    Indent,
    Dedent,
    Eof,
}
