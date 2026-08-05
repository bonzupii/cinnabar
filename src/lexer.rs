use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub col: usize,
}

impl Span {
    pub fn new(start: usize, end: usize, line: usize, col: usize) -> Self {
        Self { start, end, line, col }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    // Keywords
    Pub,
    Const,
    Val,
    Var,
    Type,
    End,
    Mod,
    Native,
    Fun,
    Impure,
    Try,
    Return,
    Match,
    If,
    Else,
    While,
    Break,
    Continue,
    Use,
    As,
    Trait,
    Impl,
    For,
    Mut,

    // Identifiers
    SnakeIdent(String),
    PascalIdent(String),
    ScreamingIdent(String),

    // Literals
    IntLit(i64),
    HexLit(u64),
    BoolLit(bool),

    // Symbols & Operators
    Colon,
    Semicolon,
    Comma,
    Dot,
    DotDot,
    At,
    FatArrow,
    Ampersand,
    Pipe,
    Caret,
    Shl,
    Shr,
    Eq,
    EqEq,
    Not,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    Plus,
    Minus,
    Star,
    Slash,
    AmpAmp,
    PipePipe,

    // Delimiters
    LParen,
    RParen,
    LBracket,
    RBracket,

    // Comments
    DocComment(String),

    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct LexerError {
    pub message: String,
    pub span: Span,
}

impl fmt::Display for LexerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Lexical Error at line {}, col {}: {}",
            self.span.line, self.span.col, self.message
        )
    }
}

pub struct Lexer<'a> {
    source: &'a str,
    chars: Vec<(usize, char)>,
    cursor: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        let chars: Vec<(usize, char)> = source.char_indices().collect();
        Self {
            source,
            chars,
            cursor: 0,
            line: 1,
            col: 1,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexerError> {
        let mut tokens = Vec::new();

        loop {
            self.skip_whitespace();
            if self.is_at_end() {
                tokens.push(Token {
                    kind: TokenKind::Eof,
                    span: Span::new(self.source.len(), self.source.len(), self.line, self.col),
                });
                break;
            }

            if let Some(doc) = self.try_lex_comment()? {
                if let Some(doc_token) = doc {
                    tokens.push(doc_token);
                }
                continue;
            }

            let start_pos = self.current_pos();
            let start_line = self.line;
            let start_col = self.col;

            let ch = match self.peek() {
                Some(character) => character,
                None => break,
            };

            let kind = match ch {
                '(' => { self.advance(); TokenKind::LParen }
                ')' => { self.advance(); TokenKind::RParen }
                '[' => { self.advance(); TokenKind::LBracket }
                ']' => { self.advance(); TokenKind::RBracket }
                ':' => { self.advance(); TokenKind::Colon }
                ';' => { self.advance(); TokenKind::Semicolon }
                ',' => { self.advance(); TokenKind::Comma }
                '@' => { self.advance(); TokenKind::At }
                '+' => { self.advance(); TokenKind::Plus }
                '-' => { self.advance(); TokenKind::Minus }
                '*' => { self.advance(); TokenKind::Star }
                '/' => { self.advance(); TokenKind::Slash }
                '^' => { self.advance(); TokenKind::Caret }

                '.' => {
                    self.advance();
                    if self.match_char('.') {
                        TokenKind::DotDot
                    } else {
                        TokenKind::Dot
                    }
                }

                '=' => {
                    self.advance();
                    if self.match_char('>') {
                        TokenKind::FatArrow
                    } else if self.match_char('=') {
                        TokenKind::EqEq
                    } else {
                        TokenKind::Eq
                    }
                }

                '!' => {
                    self.advance();
                    if self.match_char('=') {
                        TokenKind::NotEq
                    } else {
                        TokenKind::Not
                    }
                }

                '<' => {
                    self.advance();
                    if self.match_char('<') {
                        TokenKind::Shl
                    } else if self.match_char('=') {
                        TokenKind::LtEq
                    } else {
                        TokenKind::Lt
                    }
                }

                '>' => {
                    self.advance();
                    if self.match_char('>') {
                        TokenKind::Shr
                    } else if self.match_char('=') {
                        TokenKind::GtEq
                    } else {
                        TokenKind::Gt
                    }
                }

                '&' => {
                    self.advance();
                    if self.match_char('&') {
                        TokenKind::AmpAmp
                    } else {
                        TokenKind::Ampersand
                    }
                }

                '|' => {
                    self.advance();
                    if self.match_char('|') {
                        TokenKind::PipePipe
                    } else {
                        TokenKind::Pipe
                    }
                }

                '0'..='9' => self.lex_number(start_line, start_col)?,
                'a'..='z' | 'A'..='Z' => self.lex_identifier_or_keyword(start_line, start_col)?,

                unhandled_char => return Err(self.error(&format!("Unexpected character '{}'", unhandled_char))),
            };

            let end_pos = self.current_pos();
            tokens.push(Token {
                kind,
                span: Span::new(start_pos, end_pos, start_line, start_col),
            });
        }

        Ok(tokens)
    }

    fn lex_identifier_or_keyword(&mut self, start_line: usize, start_col: usize) -> Result<TokenKind, LexerError> {
        let start_pos = self.current_pos();
        while let Some(character) = self.peek() {
            if character.is_ascii_alphanumeric() || character == '_' {
                self.advance();
            } else {
                break;
            }
        }
        let end_pos = self.current_pos();
        let ident_str = &self.source[start_pos..end_pos];

        if let Some(keyword_kind) = self.lookup_keyword(ident_str) {
            return Ok(keyword_kind);
        }

        let first = match ident_str.chars().next() {
            Some(character) => character,
            None => return Err(LexerError {
                message: "Empty identifier string encountered".to_string(),
                span: Span::new(start_pos, end_pos, start_line, start_col),
            }),
        };

        if first.is_ascii_lowercase() {
            for character in ident_str.chars() {
                if !character.is_ascii_lowercase() && !character.is_ascii_digit() && character != '_' {
                    return Err(LexerError {
                        message: format!("Identifier '{}' violates snake_case rule", ident_str),
                        span: Span::new(start_pos, end_pos, start_line, start_col),
                    });
                }
            }
            Ok(TokenKind::SnakeIdent(ident_str.to_string()))
        } else if first.is_ascii_uppercase() {
            if ident_str.contains('_') {
                for character in ident_str.chars() {
                    if !character.is_ascii_uppercase() && !character.is_ascii_digit() && character != '_' {
                        return Err(LexerError {
                            message: format!("Constant '{}' violates SCREAMING_SNAKE_CASE rule", ident_str),
                            span: Span::new(start_pos, end_pos, start_line, start_col),
                        });
                    }
                }
                Ok(TokenKind::ScreamingIdent(ident_str.to_string()))
            } else {
                for character in ident_str.chars() {
                    if !character.is_ascii_alphanumeric() {
                        return Err(LexerError {
                            message: format!("Type/Module '{}' violates PascalCase rule", ident_str),
                            span: Span::new(start_pos, end_pos, start_line, start_col),
                        });
                    }
                }
                Ok(TokenKind::PascalIdent(ident_str.to_string()))
            }
        } else {
            Err(LexerError {
                message: format!("Invalid identifier start character '{}'", first),
                span: Span::new(start_pos, end_pos, start_line, start_col),
            })
        }
    }

    fn lookup_keyword(&self, ident_str: &str) -> Option<TokenKind> {
        if ident_str == "pub" { Some(TokenKind::Pub) }
        else if ident_str == "const" { Some(TokenKind::Const) }
        else if ident_str == "val" { Some(TokenKind::Val) }
        else if ident_str == "var" { Some(TokenKind::Var) }
        else if ident_str == "type" { Some(TokenKind::Type) }
        else if ident_str == "end" { Some(TokenKind::End) }
        else if ident_str == "mod" { Some(TokenKind::Mod) }
        else if ident_str == "native" { Some(TokenKind::Native) }
        else if ident_str == "fun" { Some(TokenKind::Fun) }
        else if ident_str == "impure" { Some(TokenKind::Impure) }
        else if ident_str == "try" { Some(TokenKind::Try) }
        else if ident_str == "return" { Some(TokenKind::Return) }
        else if ident_str == "match" { Some(TokenKind::Match) }
        else if ident_str == "if" { Some(TokenKind::If) }
        else if ident_str == "else" { Some(TokenKind::Else) }
        else if ident_str == "while" { Some(TokenKind::While) }
        else if ident_str == "break" { Some(TokenKind::Break) }
        else if ident_str == "continue" { Some(TokenKind::Continue) }
        else if ident_str == "use" { Some(TokenKind::Use) }
        else if ident_str == "as" { Some(TokenKind::As) }
        else if ident_str == "trait" { Some(TokenKind::Trait) }
        else if ident_str == "impl" { Some(TokenKind::Impl) }
        else if ident_str == "for" { Some(TokenKind::For) }
        else if ident_str == "mut" { Some(TokenKind::Mut) }
        else if ident_str == "true" { Some(TokenKind::BoolLit(true)) }
        else if ident_str == "false" { Some(TokenKind::BoolLit(false)) }
        else { None }
    }

    fn lex_number(&mut self, start_line: usize, start_col: usize) -> Result<TokenKind, LexerError> {
        let start_pos = self.current_pos();

        if self.peek() == Some('0') {
            self.advance();
            if self.peek() == Some('x') || self.peek() == Some('X') {
                self.advance();
                let hex_start = self.current_pos();
                while let Some(character) = self.peek() {
                    if character.is_ascii_hexdigit() {
                        self.advance();
                    } else {
                        break;
                    }
                }
                let hex_str = &self.source[hex_start..self.current_pos()];
                let val = u64::from_str_radix(hex_str, 16).map_err(|parse_err| LexerError {
                    message: format!("Invalid hex literal '0x{}': {}", hex_str, parse_err),
                    span: Span::new(start_pos, self.current_pos(), start_line, start_col),
                })?;
                return Ok(TokenKind::HexLit(val));
            }
        }

        while let Some(character) = self.peek() {
            if character.is_ascii_digit() {
                self.advance();
            } else {
                break;
            }
        }

        let num_str = &self.source[start_pos..self.current_pos()];
        let val = num_str.parse::<i64>().map_err(|parse_err| LexerError {
            message: format!("Invalid integer literal '{}': {}", num_str, parse_err),
            span: Span::new(start_pos, self.current_pos(), start_line, start_col),
        })?;

        Ok(TokenKind::IntLit(val))
    }

    fn try_lex_comment(&mut self) -> Result<Option<Option<Token>>, LexerError> {
        if self.peek() != Some('#') {
            return Ok(None);
        }

        let start_pos = self.current_pos();
        let start_line = self.line;
        let start_col = self.col;
        self.advance();

        if self.peek() == Some('!') {
            self.advance();

            if self.peek() == Some('|') {
                self.advance();
                let content_start = self.current_pos();

                while !self.is_at_end() {
                    if self.peek() == Some('|') && self.peek_next() == Some('#') {
                        let content = self.source[content_start..self.current_pos()].trim().to_string();
                        self.advance();
                        self.advance();
                        return Ok(Some(Some(Token {
                            kind: TokenKind::DocComment(content),
                            span: Span::new(start_pos, self.current_pos(), start_line, start_col),
                        })));
                    }

                    if self.peek() == Some('#') && self.peek_next() == Some('|') {
                        return Err(self.error("Nested block comments are not allowed"));
                    }

                    if self.peek() == Some('#')
                        && self.peek_next() == Some('!')
                        && self.peek_next_next() == Some('|')
                    {
                        return Err(self.error("Nested doc block comments are not allowed"));
                    }

                    self.advance();
                }

                return Err(self.error("Unterminated block doc comment"));
            }

            let content_start = self.current_pos();
            while let Some(character) = self.peek() {
                if character == '\n' {
                    break;
                }
                self.advance();
            }

            let content = self.source[content_start..self.current_pos()].trim().to_string();
            return Ok(Some(Some(Token {
                kind: TokenKind::DocComment(content),
                span: Span::new(start_pos, self.current_pos(), start_line, start_col),
            })));
        }

        if self.peek() == Some('|') {
            self.advance();

            while !self.is_at_end() {
                if self.peek() == Some('|') && self.peek_next() == Some('#') {
                    self.advance();
                    self.advance();
                    return Ok(Some(None));
                }

                if self.peek() == Some('#') && self.peek_next() == Some('|') {
                    return Err(self.error("Nested block comments are not allowed"));
                }

                if self.peek() == Some('#')
                    && self.peek_next() == Some('!')
                    && self.peek_next_next() == Some('|')
                {
                    return Err(self.error("Nested doc block comments are not allowed"));
                }

                self.advance();
            }

            return Err(self.error("Unterminated block comment"));
        }

        while let Some(character) = self.peek() {
            if character == '\n' {
                break;
            }
            self.advance();
        }

        Ok(Some(None))
    }

    fn skip_whitespace(&mut self) {
        while let Some(character) = self.peek() {
            if character == ' ' || character == '\t' || character == '\r' || character == '\n' {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn advance(&mut self) -> Option<char> {
        if self.cursor >= self.chars.len() {
            return None;
        }
        let character = self.chars[self.cursor].1;
        self.cursor += 1;
        if character == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(character)
    }

    fn match_char(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<char> {
        if self.cursor < self.chars.len() {
            Some(self.chars[self.cursor].1)
        } else {
            None
        }
    }

    fn peek_next(&self) -> Option<char> {
        if self.cursor + 1 < self.chars.len() {
            Some(self.chars[self.cursor + 1].1)
        } else {
            None
        }
    }

    fn peek_next_next(&self) -> Option<char> {
        if self.cursor + 2 < self.chars.len() {
            Some(self.chars[self.cursor + 2].1)
        } else {
            None
        }
    }

    fn current_pos(&self) -> usize {
        if self.cursor < self.chars.len() {
            self.chars[self.cursor].0
        } else {
            self.source.len()
        }
    }

    fn is_at_end(&self) -> bool {
        self.cursor >= self.chars.len()
    }

    fn error(&self, message: &str) -> LexerError {
        LexerError {
            message: message.to_string(),
            span: Span::new(self.current_pos(), self.current_pos(), self.line, self.col),
        }
    }
}
