/// Token types for the Spectre language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Ident(String),
    IntLiteral(String),
    FloatLiteral(String),
    StringLiteral(String),
    CharLiteral(String),
    RawStringLiteral(String),
    KwFn,
    KwVal,
    KwType,
    KwExtern,
    KwPub,
    KwMut,
    KwSelf,
    KwIf,
    KwElif,
    KwElse,
    KwFor,
    KwIn,
    KwBreak,
    KwContinue,
    KwReturn,
    KwMatch,
    KwWhen,
    KwOtherwise,
    KwSome,
    KwNone,
    KwOk,
    KwErr,
    KwTrust,
    KwGuarded,
    KwTest,
    KwAssert,
    KwVoid,
    KwBool,
    KwRef,
    KwUnion,
    KwEnum,
    KwDeref,
    KwAddr,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    EqEq,
    Bang,
    BangEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    AndAnd,
    OrOr,
    BangBang,
    Question,
    Dot,
    DotDot,
    Comma,
    Colon,
    Semi,
    Arrow,
    FatArrow,
    At,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    KwPre,
    KwPost,
    BacktickString(String),
    Whitespace,
    Comment,
    Unknown(char),
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub col: usize,
}

impl Token {
    pub fn span(&self) -> std::ops::Range<usize> {
        self.start..self.end
    }
}

pub struct Lexer {
    src: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
}

impl Lexer {
    pub fn new(src: &str) -> Self {
        Self {
            src: src.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.src.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.src.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        if self.pos < self.src.len() {
            let c = self.src[self.pos];
            self.pos += 1;
            if c == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            Some(c)
        } else {
            None
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn skip_line_comment(&mut self) {
        while let Some(c) = self.peek() {
            self.advance();
            if c == '\n' {
                break;
            }
        }
    }

    fn skip_block_comment(&mut self) {
        self.advance(); // /
        self.advance(); // *
        let mut depth = 1;
        while depth > 0 {
            match self.peek() {
                Some('*') => {
                    self.advance();
                    if self.peek() == Some('/') {
                        self.advance();
                        depth -= 1;
                    }
                }
                Some(_) => {
                    self.advance();
                }
                None => break,
            }
        }
    }

    fn read_ident(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        s
    }

    fn read_number(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        s
    }

    fn read_string(&mut self) -> String {
        let mut s = String::new();
        self.advance(); // opening "
        while let Some(c) = self.advance() {
            if c == '\\' {
                if let Some(escaped) = self.advance() {
                    s.push('\\');
                    s.push(escaped);
                }
            } else if c == '"' {
                break;
            } else {
                s.push(c);
            }
        }
        s
    }

    fn read_char_literal(&mut self) -> String {
        let mut s = String::new();
        self.advance(); // opening '
        while let Some(c) = self.advance() {
            if c == '\\' {
                if let Some(escaped) = self.advance() {
                    s.push('\\');
                    s.push(escaped);
                }
            } else if c == '\'' {
                break;
            } else {
                s.push(c);
            }
        }
        s
    }

    fn read_backtick_string(&mut self) -> String {
        let mut s = String::new();
        self.advance(); // opening `
        while let Some(c) = self.advance() {
            if c == '`' {
                break;
            } else {
                s.push(c);
            }
        }
        s
    }

    fn keyword_or_ident(s: &str) -> TokenKind {
        match s {
            "fn" => TokenKind::KwFn,
            "val" => TokenKind::KwVal,
            "type" => TokenKind::KwType,
            "extern" => TokenKind::KwExtern,
            "pub" => TokenKind::KwPub,
            "mut" => TokenKind::KwMut,
            "self" => TokenKind::KwSelf,
            "if" => TokenKind::KwIf,
            "elif" => TokenKind::KwElif,
            "else" => TokenKind::KwElse,
            "for" => TokenKind::KwFor,
            "in" => TokenKind::KwIn,
            "break" => TokenKind::KwBreak,
            "continue" => TokenKind::KwContinue,
            "return" => TokenKind::KwReturn,
            "match" => TokenKind::KwMatch,
            "when" => TokenKind::KwWhen,
            "otherwise" => TokenKind::KwOtherwise,
            "some" => TokenKind::KwSome,
            "none" => TokenKind::KwNone,
            "ok" => TokenKind::KwOk,
            "err" => TokenKind::KwErr,
            "trust" => TokenKind::KwTrust,
            "guarded" => TokenKind::KwGuarded,
            "test" => TokenKind::KwTest,
            "assert" => TokenKind::KwAssert,
            "void" => TokenKind::KwVoid,
            "bool" => TokenKind::KwBool,
            "ref" => TokenKind::KwRef,
            "union" => TokenKind::KwUnion,
            "enum" => TokenKind::KwEnum,
            "deref" => TokenKind::KwDeref,
            "addr" => TokenKind::KwAddr,
            "pre" => TokenKind::KwPre,
            "post" => TokenKind::KwPost,
            _ => TokenKind::Ident(s.to_string()),
        }
    }

    pub fn next_token(&mut self) -> Option<Token> {
        self.skip_whitespace();

        if self.peek() == Some('/') && self.peek_next() == Some('/') {
            let start = self.pos;
            let line = self.line;
            let col = self.col;
            self.skip_line_comment();
            return Some(Token {
                kind: TokenKind::Comment,
                start,
                end: self.pos,
                line,
                col,
            });
        }
        if self.peek() == Some('/') && self.peek_next() == Some('*') {
            let start = self.pos;
            let line = self.line;
            let col = self.col;
            self.skip_block_comment();
            return Some(Token {
                kind: TokenKind::Comment,
                start,
                end: self.pos,
                line,
                col,
            });
        }

        if self.pos >= self.src.len() {
            return None;
        }

        let start = self.pos;
        let line = self.line;
        let col = self.col;

        let kind = match self.peek() {
            Some('(') => {
                self.advance();
                TokenKind::LParen
            }
            Some(')') => {
                self.advance();
                TokenKind::RParen
            }
            Some('{') => {
                self.advance();
                TokenKind::LBrace
            }
            Some('}') => {
                self.advance();
                TokenKind::RBrace
            }
            Some('[') => {
                self.advance();
                TokenKind::LBracket
            }
            Some(']') => {
                self.advance();
                TokenKind::RBracket
            }
            Some(',') => {
                self.advance();
                TokenKind::Comma
            }
            Some(';') => {
                self.advance();
                TokenKind::Semi
            }
            Some('@') => {
                self.advance();
                TokenKind::At
            }
            Some('?') => {
                self.advance();
                TokenKind::Question
            }

            Some('`') => {
                let s = self.read_backtick_string();
                TokenKind::BacktickString(s)
            }

            Some('"') => {
                let s = self.read_string();
                TokenKind::StringLiteral(s)
            }

            Some('\'') => {
                let s = self.read_char_literal();
                TokenKind::CharLiteral(s)
            }

            Some('+') => {
                self.advance();
                if self.peek() == Some('+') {
                    self.advance();
                    TokenKind::BangBang
                } else {
                    TokenKind::Plus
                }
            }
            Some('-') => {
                self.advance();
                if self.peek() == Some('>') {
                    self.advance();
                    TokenKind::Arrow
                } else {
                    TokenKind::Minus
                }
            }
            Some('*') => {
                self.advance();
                TokenKind::Star
            }
            Some('/') => {
                self.advance();
                TokenKind::Slash
            }
            Some('%') => {
                self.advance();
                TokenKind::Percent
            }

            Some('=') => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::EqEq
                } else if self.peek() == Some('>') {
                    self.advance();
                    TokenKind::Arrow
                } else {
                    TokenKind::Eq
                }
            }

            Some('!') => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::BangEq
                } else {
                    TokenKind::Bang
                }
            }

            Some('<') => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::LtEq
                } else {
                    TokenKind::Lt
                }
            }
            Some('>') => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::GtEq
                } else {
                    TokenKind::Gt
                }
            }

            Some('&') => {
                self.advance();
                if self.peek() == Some('&') {
                    self.advance();
                    TokenKind::AndAnd
                } else {
                    TokenKind::Unknown('&')
                }
            }
            Some('|') => {
                self.advance();
                if self.peek() == Some('|') {
                    self.advance();
                    TokenKind::OrOr
                } else {
                    TokenKind::Unknown('|')
                }
            }

            Some('.') => {
                self.advance();
                if self.peek() == Some('.') {
                    self.advance();
                    TokenKind::DotDot
                } else {
                    TokenKind::Dot
                }
            }

            Some(':') => {
                self.advance();
                if self.peek() == Some(':') {
                    self.advance();
                    TokenKind::Colon
                } else {
                    TokenKind::Colon
                }
            }

            Some(c) if c.is_ascii_digit() => {
                let num = self.read_number();
                if num.contains('.') {
                    TokenKind::FloatLiteral(num)
                } else {
                    TokenKind::IntLiteral(num)
                }
            }

            Some(c) if c.is_alphabetic() || c == '_' => {
                let ident = self.read_ident();
                Self::keyword_or_ident(&ident)
            }

            Some(c) => {
                self.advance();
                TokenKind::Unknown(c)
            }

            None => return None,
        };

        Some(Token {
            kind,
            start,
            end: self.pos,
            line,
            col,
        })
    }

    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        while let Some(tok) = self.next_token() {
            if tok.kind != TokenKind::Whitespace {
                tokens.push(tok);
            }
        }
        tokens
    }
}
