use crate::lexer::token::Span;
use std::fmt;

#[derive(Debug, Clone)]
pub struct StackFrame {
    pub name: String,
    pub span: Span,
    pub kind: StackFrameKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackFrameKind {
    Function,
    NativeFunction,
    ModuleTopLevel,
    Coroutine,
}

#[derive(Debug, Clone)]
pub enum IcooError {
    Lexer { message: String, span: Span },
    Parse { message: String, span: Span },
    Resolve { message: String, span: Span },
    Type { message: String, span: Span },
    Runtime {
        message: String,
        span: Option<Span>,
        trace: Vec<StackFrame>,
    },
    Return(crate::runtime::value::Value),
    Await(crate::runtime::value::Value),
    Break,
    Continue,
}

impl IcooError {
    pub fn lexer(message: impl Into<String>, span: Span) -> Self {
        Self::Lexer {
            message: message.into(),
            span,
        }
    }

    pub fn parse(message: impl Into<String>, span: Span) -> Self {
        Self::Parse {
            message: message.into(),
            span,
        }
    }

    pub fn resolve(message: impl Into<String>, span: Span) -> Self {
        Self::Resolve {
            message: message.into(),
            span,
        }
    }

    pub fn typecheck(message: impl Into<String>, span: Span) -> Self {
        Self::Type {
            message: message.into(),
            span,
        }
    }

    pub fn runtime(message: impl Into<String>, span: Option<Span>) -> Self {
        Self::Runtime {
            message: message.into(),
            span,
            trace: Vec::new(),
        }
    }

    pub fn push_frame(&mut self, name: String, span: Span, kind: StackFrameKind) {
        if let IcooError::Runtime { trace, .. } = self {
            trace.push(StackFrame { name, span, kind });
        }
    }
}

impl fmt::Display for IcooError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IcooError::Lexer { message, span } => {
                write!(f, "{}:{}: lexer error: {}", span.line, span.column, message)
            }
            IcooError::Parse { message, span } => {
                write!(f, "{}:{}: parse error: {}", span.line, span.column, message)
            }
            IcooError::Resolve { message, span } => {
                write!(
                    f,
                    "{}:{}: resolve error: {}",
                    span.line, span.column, message
                )
            }
            IcooError::Type { message, span } => {
                write!(f, "{}:{}: type error: {}", span.line, span.column, message)
            }
            IcooError::Runtime {
                message, span, trace,
            } => {
                if let Some(span) = span {
                    write!(
                        f,
                        "{}:{}: runtime error: {}",
                        span.line, span.column, message
                    )?;
                } else {
                    write!(f, "runtime error: {}", message)?;
                }
                for frame in trace.iter().rev() {
                    write!(
                        f,
                        "\n  at {} ({}:{})",
                        frame.name, frame.span.line, frame.span.column
                    )?;
                }
                Ok(())
            }
            IcooError::Return(_) => write!(f, "internal return signal"),
            IcooError::Await(_) => write!(f, "internal await signal"),
            IcooError::Break => write!(f, "internal break signal"),
            IcooError::Continue => write!(f, "internal continue signal"),
        }
    }
}

impl std::error::Error for IcooError {}

pub type IcooResult<T> = Result<T, IcooError>;
