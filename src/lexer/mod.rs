pub mod token;

use crate::error::{IcooError, IcooResult};
use token::{Span, Token, TokenKind};

pub fn lex(source: &str) -> IcooResult<Vec<Token>> {
    Lexer::new(source).lex()
}

struct Lexer<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    indents: Vec<usize>,
    offset: usize,
    line_starts: Vec<usize>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        let mut line_starts = vec![0];
        for (index, ch) in source.char_indices() {
            if ch == '\n' {
                line_starts.push(index + 1);
            }
        }
        Self {
            source,
            tokens: Vec::new(),
            indents: vec![0],
            offset: 0,
            line_starts,
        }
    }

    fn lex(mut self) -> IcooResult<Vec<Token>> {
        let lines: Vec<&str> = self.source.lines().collect();
        let mut line_index = 0;
        while line_index < lines.len() {
            let raw_line = lines[line_index];
            let line_no = line_index + 1;
            let line = raw_line.trim_end_matches('\r');
            let content_start = line.chars().take_while(|c| *c == ' ').count();
            let content = &line[content_start..];

            if content.trim().is_empty() || content.trim_start().starts_with('#') {
                line_index += 1;
                self.offset = self
                    .line_starts
                    .get(line_index)
                    .copied()
                    .unwrap_or(self.source.len());
                continue;
            }

            self.handle_indent(content_start, line_no)?;
            let consumed_until = self.scan_line(content, line_no, content_start + 1)?;
            self.push(
                TokenKind::Newline,
                line_no,
                line.len() + 1,
                line.len(),
                line.len(),
            );
            if let Some(end_abs) = consumed_until {
                line_index = self.line_for_offset(end_abs);
                if end_abs > *self.line_starts.get(line_index).unwrap_or(&0)
                    && line_index + 1 < lines.len()
                {
                    line_index += 1;
                }
            } else {
                line_index += 1;
            }
            self.offset = self
                .line_starts
                .get(line_index)
                .copied()
                .unwrap_or(self.source.len());
        }

        let line = self.source.lines().count().max(1);
        while self.indents.len() > 1 {
            self.indents.pop();
            self.push(
                TokenKind::Dedent,
                line,
                1,
                self.source.len(),
                self.source.len(),
            );
        }
        self.push(
            TokenKind::Eof,
            line,
            1,
            self.source.len(),
            self.source.len(),
        );
        Ok(self.tokens)
    }

    fn handle_indent(&mut self, indent: usize, line: usize) -> IcooResult<()> {
        let current = *self.indents.last().unwrap();
        if indent > current {
            self.indents.push(indent);
            self.push(
                TokenKind::Indent,
                line,
                1,
                self.offset,
                self.offset + indent,
            );
        } else if indent < current {
            while indent < *self.indents.last().unwrap() {
                self.indents.pop();
                self.push(
                    TokenKind::Dedent,
                    line,
                    1,
                    self.offset,
                    self.offset + indent,
                );
            }
            if indent != *self.indents.last().unwrap() {
                return Err(IcooError::lexer(
                    "indentation does not match any outer indentation level",
                    Span::new(line, 1, self.offset, self.offset + indent),
                ));
            }
        }
        Ok(())
    }

    fn scan_line(
        &mut self,
        line: &str,
        line_no: usize,
        base_col: usize,
    ) -> IcooResult<Option<usize>> {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let col = base_col + i;
            match c {
                ' ' | '\t' => i += 1,
                '#' => break,
                '(' => {
                    self.add(TokenKind::LeftParen, line_no, col, i, i + 1);
                    i += 1;
                }
                ')' => {
                    self.add(TokenKind::RightParen, line_no, col, i, i + 1);
                    i += 1;
                }
                '[' => {
                    self.add(TokenKind::LeftBracket, line_no, col, i, i + 1);
                    i += 1;
                }
                ']' => {
                    self.add(TokenKind::RightBracket, line_no, col, i, i + 1);
                    i += 1;
                }
                '{' => {
                    self.add(TokenKind::LeftBrace, line_no, col, i, i + 1);
                    i += 1;
                }
                '}' => {
                    self.add(TokenKind::RightBrace, line_no, col, i, i + 1);
                    i += 1;
                }
                ',' => {
                    self.add(TokenKind::Comma, line_no, col, i, i + 1);
                    i += 1;
                }
                '.' => {
                    self.add(TokenKind::Dot, line_no, col, i, i + 1);
                    i += 1;
                }
                ':' => {
                    self.add(TokenKind::Colon, line_no, col, i, i + 1);
                    i += 1;
                }
                '+' => {
                    self.add(TokenKind::Plus, line_no, col, i, i + 1);
                    i += 1;
                }
                '*' => {
                    self.add(TokenKind::Star, line_no, col, i, i + 1);
                    i += 1;
                }
                '/' => {
                    self.add(TokenKind::Slash, line_no, col, i, i + 1);
                    i += 1;
                }
                '%' => {
                    self.add(TokenKind::Percent, line_no, col, i, i + 1);
                    i += 1;
                }
                '-' if Self::matches(&chars, i + 1, '>') => {
                    self.add(TokenKind::Arrow, line_no, col, i, i + 2);
                    i += 2;
                }
                '-' => {
                    self.add(TokenKind::Minus, line_no, col, i, i + 1);
                    i += 1;
                }
                '<' if Self::matches(&chars, i + 1, '-') => {
                    self.add(TokenKind::LeftArrow, line_no, col, i, i + 2);
                    i += 2;
                }
                '<' if Self::matches(&chars, i + 1, '=') => {
                    self.add(TokenKind::LessEqual, line_no, col, i, i + 2);
                    i += 2;
                }
                '<' => {
                    self.add(TokenKind::Less, line_no, col, i, i + 1);
                    i += 1;
                }
                '>' if Self::matches(&chars, i + 1, '=') => {
                    self.add(TokenKind::GreaterEqual, line_no, col, i, i + 2);
                    i += 2;
                }
                '>' => {
                    self.add(TokenKind::Greater, line_no, col, i, i + 1);
                    i += 1;
                }
                '=' if Self::matches(&chars, i + 1, '=') => {
                    self.add(TokenKind::EqualEqual, line_no, col, i, i + 2);
                    i += 2;
                }
                '=' => {
                    self.add(TokenKind::Equal, line_no, col, i, i + 1);
                    i += 1;
                }
                '!' if Self::matches(&chars, i + 1, '=') => {
                    self.add(TokenKind::BangEqual, line_no, col, i, i + 2);
                    i += 2;
                }
                '"' if Self::starts_with(&chars, i, "\"\"\"") => {
                    let end_abs = self.multiline_string(line_no, base_col + i, i, false)?;
                    return Ok(Some(end_abs));
                }
                '"' => {
                    i = self.string(&chars, line_no, base_col, i, false)?;
                }
                'f' if Self::matches(&chars, i + 1, '"')
                    && Self::starts_with(&chars, i + 1, "\"\"\"") =>
                {
                    let end_abs = self.multiline_string(line_no, base_col + i, i + 1, true)?;
                    return Ok(Some(end_abs));
                }
                'f' if Self::matches(&chars, i + 1, '"') => {
                    i = self.string(&chars, line_no, base_col, i + 1, true)?;
                }
                c if c.is_ascii_digit() => {
                    i = self.number(&chars, line_no, base_col, i);
                }
                c if is_ident_start(c) => {
                    i = self.identifier(&chars, line_no, base_col, i);
                }
                _ => {
                    return Err(IcooError::lexer(
                        format!("unexpected character '{}'", c),
                        Span::new(line_no, col, self.offset + i, self.offset + i + 1),
                    ));
                }
            }
        }
        Ok(None)
    }

    fn string(
        &mut self,
        chars: &[char],
        line_no: usize,
        base_col: usize,
        start: usize,
        template: bool,
    ) -> IcooResult<usize> {
        let mut i = start + 1;
        let mut value = String::new();
        while i < chars.len() {
            match chars[i] {
                '"' => {
                    let kind = if template {
                        TokenKind::TemplateString(value)
                    } else {
                        TokenKind::String(value)
                    };
                    let token_start = if template { start - 1 } else { start };
                    self.add(kind, line_no, base_col + token_start, token_start, i + 1);
                    return Ok(i + 1);
                }
                '\\' if i + 1 < chars.len() => {
                    i += 1;
                    value.push(match chars[i] {
                        'n' => '\n',
                        't' => '\t',
                        '"' => '"',
                        '\\' => '\\',
                        other => other,
                    });
                }
                c => value.push(c),
            }
            i += 1;
        }
        Err(IcooError::lexer(
            "unterminated string",
            Span::new(
                line_no,
                base_col + start,
                self.offset + start,
                self.offset + chars.len(),
            ),
        ))
    }

    fn number(&mut self, chars: &[char], line_no: usize, base_col: usize, start: usize) -> usize {
        let mut i = start;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        let mut is_float = false;
        if i + 1 < chars.len() && chars[i] == '.' && chars[i + 1].is_ascii_digit() {
            is_float = true;
            i += 1;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
        }
        let text: String = chars[start..i].iter().collect();
        let kind = if is_float {
            TokenKind::Float(text.parse().unwrap())
        } else {
            TokenKind::Int(text.parse().unwrap())
        };
        self.add(kind, line_no, base_col + start, start, i);
        i
    }

    fn identifier(
        &mut self,
        chars: &[char],
        line_no: usize,
        base_col: usize,
        start: usize,
    ) -> usize {
        let mut i = start;
        while i < chars.len() && is_ident_part(chars[i]) {
            i += 1;
        }
        let text: String = chars[start..i].iter().collect();
        self.add(keyword_or_ident(text), line_no, base_col + start, start, i);
        i
    }

    fn matches(chars: &[char], i: usize, expected: char) -> bool {
        chars.get(i).copied() == Some(expected)
    }

    fn starts_with(chars: &[char], i: usize, value: &str) -> bool {
        value
            .chars()
            .enumerate()
            .all(|(offset, expected)| chars.get(i + offset).copied() == Some(expected))
    }

    fn multiline_string(
        &mut self,
        line_no: usize,
        column: usize,
        quote_start: usize,
        template: bool,
    ) -> IcooResult<usize> {
        let abs_quote_start = self.offset + quote_start;
        let content_start = abs_quote_start + 3;
        let Some(relative_end) = self.source[content_start..].find("\"\"\"") else {
            return Err(IcooError::lexer(
                "unterminated multiline string",
                Span::new(line_no, column, abs_quote_start, self.source.len()),
            ));
        };
        let content_end = content_start + relative_end;
        let token_end = content_end + 3;
        let value = self.source[content_start..content_end]
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        let kind = if template {
            TokenKind::TemplateString(value)
        } else {
            TokenKind::String(value)
        };
        let token_start = if template {
            abs_quote_start - 1
        } else {
            abs_quote_start
        };
        self.tokens.push(Token {
            kind,
            span: Span::new(line_no, column, token_start, token_end),
        });
        Ok(token_end)
    }

    fn line_for_offset(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        }
    }

    fn add(&mut self, kind: TokenKind, line: usize, column: usize, start: usize, end: usize) {
        self.push(kind, line, column, self.offset + start, self.offset + end);
    }

    fn push(&mut self, kind: TokenKind, line: usize, column: usize, start: usize, end: usize) {
        self.tokens.push(Token {
            kind,
            span: Span::new(line, column, start, end),
        });
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_part(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn keyword_or_ident(text: String) -> TokenKind {
    match text.as_str() {
        "let" => TokenKind::Let,
        "const" => TokenKind::Const,
        "final" => TokenKind::Final,
        "async" => TokenKind::Async,
        "co" => TokenKind::Co,
        "fn" => TokenKind::Fn,
        "class" => TokenKind::Class,
        "if" => TokenKind::If,
        "elif" => TokenKind::Elif,
        "else" => TokenKind::Else,
        "while" => TokenKind::While,
        "return" => TokenKind::Return,
        "break" => TokenKind::Break,
        "continue" => TokenKind::Continue,
        "yield" => TokenKind::Yield,
        "await" => TokenKind::Await,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "nil" => TokenKind::Nil,
        "self" => TokenKind::Self_,
        "super" => TokenKind::Super,
        "and" => TokenKind::And,
        "or" => TokenKind::Or,
        "not" => TokenKind::Not,
        _ => TokenKind::Ident(text),
    }
}
