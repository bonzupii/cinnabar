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

    EOF,
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
                    kind: TokenKind::EOF,
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
                '-' => { self.advance(); TokenKind::Minus } // Always emit Minus!
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

        match ident_str {
            "pub" => return Ok(TokenKind::Pub),
            "const" => return Ok(TokenKind::Const),
            "val" => return Ok(TokenKind::Val),
            "var" => return Ok(TokenKind::Var),
            "type" => return Ok(TokenKind::Type),
            "end" => return Ok(TokenKind::End),
            "mod" => return Ok(TokenKind::Mod),
            "native" => return Ok(TokenKind::Native),
            "fun" => return Ok(TokenKind::Fun),
            "impure" => return Ok(TokenKind::Impure),
            "try" => return Ok(TokenKind::Try),
            "return" => return Ok(TokenKind::Return),
            "match" => return Ok(TokenKind::Match),
            "if" => return Ok(TokenKind::If),
            "else" => return Ok(TokenKind::Else),
            "while" => return Ok(TokenKind::While),
            "break" => return Ok(TokenKind::Break),
            "continue" => return Ok(TokenKind::Continue),
            "use" => return Ok(TokenKind::Use),
            "as" => return Ok(TokenKind::As),
            "trait" => return Ok(TokenKind::Trait),
            "impl" => return Ok(TokenKind::Impl),
            "for" => return Ok(TokenKind::For),
            "mut" => return Ok(TokenKind::Mut),
            "true" => return Ok(TokenKind::BoolLit(true)),
            "false" => return Ok(TokenKind::BoolLit(false)),
            _user_ident => {}
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
        let (_, character) = self.chars[self.cursor];
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
