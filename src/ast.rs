use crate::lexer::{Token, TokenKind};
use std::ops::Range;

/// A span of source code.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub col: usize,
}

impl Span {
    pub fn range(&self) -> Range<usize> {
        self.start..self.end
    }
}

impl From<&Token> for Span {
    fn from(t: &Token) -> Self {
        Span {
            start: t.start,
            end: t.end,
            line: t.line,
            col: t.col,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Named(String, Span),                              // e.g. i32, SomeType
    Generic(String, Vec<Type>, Span), // e.g. option[i32], list[ref char], result[i64, ParseError]
    Ref(Box<Type>, Span),             // ref T
    Mut(Box<Type>, Span),             // mut T
    Fn(Vec<(Option<String>, Type)>, Box<Type>, Span), // fn(args) -> ret
    Unit(Span),                       // void / ()
    Bool(Span),
    Never(Span),
}

impl Type {
    pub fn display(&self) -> String {
        match self {
            Type::Named(n, _) => n.clone(),
            Type::Generic(n, args, _) => {
                format!(
                    "{}[{}]",
                    n,
                    args.iter()
                        .map(|a| a.display())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Type::Ref(inner, _) => format!("ref {}", inner.display()),
            Type::Mut(inner, _) => format!("mut {}", inner.display()),
            Type::Fn(args, ret, _) => {
                let arg_str = args
                    .iter()
                    .map(|(name, ty)| match name {
                        Some(n) => format!("{}: {}", n, ty.display()),
                        None => ty.display(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("fn({}) -> {}", arg_str, ret.display())
            }
            Type::Unit(_) => "void".to_string(),
            Type::Bool(_) => "bool".to_string(),
            Type::Never(_) => "never".to_string(),
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Type::Named(_, s) => s.clone(),
            Type::Generic(_, _, s) => s.clone(),
            Type::Ref(_, s) => s.clone(),
            Type::Mut(_, s) => s.clone(),
            Type::Fn(_, _, s) => s.clone(),
            Type::Unit(s) => s.clone(),
            Type::Bool(s) => s.clone(),
            Type::Never(s) => s.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Condition {
    pub name: String,
    pub name_span: Span,
    pub expr: Box<Expr>,
}

#[derive(Debug, Clone)]
pub struct ConditionBlock {
    pub conditions: Vec<Condition>,
    pub guarded: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Ident(String, Span),
    IntLiteral(String, Span),
    FloatLiteral(String, Span),
    StringLiteral(String, Span),
    BoolLiteral(bool, Span),
    NoneLiteral(Span),

    BinaryOp(Box<Expr>, String, Box<Expr>, Span),
    UnaryOp(String, Box<Expr>, Span),
    Call(Box<Expr>, Vec<Expr>, Span),
    MethodCall(Box<Expr>, String, Vec<Expr>, Span),
    FieldAccess(Box<Expr>, String, Span),
    Index(Box<Expr>, Box<Expr>, Span),
    Cast(Box<Expr>, Type, Span), // expr as Type
    Trust(Box<Expr>, Span),      // trust expr
    Return(Option<Box<Expr>>, Span),
    If(Box<Expr>, Box<Expr>, Option<Box<Expr>>, Span), // cond, then, else_opt
    ForLoop(Option<ForLoopKind>, Box<Expr>, Box<Expr>, Span),
    Match(Box<Expr>, Vec<MatchArm>, Span),
    Block(Vec<Stmt>, Span),
    StructLit(Vec<(String, Expr)>, Span),
    ArrayLit(Vec<Expr>, Span),
    SomeVariant(Box<Expr>, Span), // some expr
    OkVariant(Box<Expr>, Span),
    ErrVariant(Box<Expr>, Span),
    WhenExpr(String, Box<Expr>, Span), // when platform { ... }
    Deref(Box<Expr>, Span),
    AddrOf(Box<Expr>, Span),
    Propagate(Box<Expr>, Span), // expr?
    Grouped(Box<Expr>, Span),
    Intrinsic(String, Vec<Expr>, Span), // @alloc, @memset, etc.
}

#[derive(Debug, Clone)]
pub enum ForLoopKind {
    CStyle,   // for (i = 0; i < 10; i++)
    ForIn,    // for x in xs
    Infinite, // for { ... }
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Ident(String, Span),
    Discard(Span),               // _
    SomePat(Box<Pattern>, Span), // some x
    OkPat(Box<Pattern>, Span),   // ok x
    ErrPat(Box<Pattern>, Span),  // err e
    TypePat(String, Span),       // i32, Int32, etc.
    StringPat(String, Span),     // "hello"
    ElsePat(Span),               // else
    TuplePat(Vec<Pattern>, Span),
    ConstructPat(String, Vec<Pattern>, Span),
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let(String, Option<Type>, Box<Expr>, Span, bool), // name, type_opt, expr, span, is_mut
    Expr(Box<Expr>, Span),
    Assign(Box<Expr>, Box<Expr>, Span),
    Use(String, String, Span), // local_name, module_path, span
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub name_span: Span,
    pub self_type: Option<String>, // e.g. SomeType for fn (SomeType) method
    pub self_mut: bool,
    pub params: Vec<Param>,
    pub return_type: Type,
    pub returns_untrusted: bool, // trailing ! on return type
    pub pre: Option<ConditionBlock>,
    pub post: Option<ConditionBlock>,
    pub body: Option<Expr>,
    pub is_pub: bool,
    pub is_extern: bool,
    pub extern_abi: Option<String>,  // e.g. "C"
    pub extern_name: Option<String>, // e.g. "malloc"
    pub span: Span,
    pub doc_comments: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TypeDef {
    pub name: String,
    pub name_span: Span,
    pub kind: TypeDefKind,
    pub is_pub: bool,
    pub is_extern: bool,
    pub span: Span,
    pub doc_comments: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum TypeDefKind {
    Struct(Vec<StructField>),          // { x: i32, y: mut i32 }
    Union(Vec<Type>),                  // i32 | i64 | ref char
    UnionConstruct(Vec<UnionVariant>), // Int32(i32) | Str(String)
    Enum(Vec<EnumVariant>),
}

#[derive(Debug, Clone)]
pub struct StructField {
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub is_mut: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct UnionVariant {
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub name_span: Span,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TestBlock {
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Item {
    Function(Function),
    TypeDef(TypeDef),
    Use(String, String, Span), // local_name, module_path
    Test(TestBlock),
}

#[derive(Debug, Clone)]
pub struct Module {
    pub items: Vec<Item>,
    pub source: String,
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    source: String,
}

impl Parser {
    pub fn new(tokens: Vec<Token>, source: String) -> Self {
        Self {
            tokens,
            pos: 0,
            source,
        }
    }

    fn peek(&self) -> Option<&TokenKind> {
        self.tokens.get(self.pos).map(|t| &t.kind)
    }

    fn peek_token(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<Token> {
        if self.pos < self.tokens.len() {
            let t = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(t)
        } else {
            None
        }
    }

    fn expect(&mut self, _expected: &str) -> Option<Token> {
        if let Some(tok) = self.advance() {
            Some(tok)
        } else {
            None
        }
    }

    fn skip_comments(&mut self) {
        while let Some(t) = self.peek_token() {
            if t.kind == TokenKind::Comment {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn parse_type(&mut self) -> Option<Type> {
        self.skip_comments();

        if let Some(TokenKind::KwMut) = self.peek() {
            let t = self.advance().unwrap();
            let span = Span::from(&t);
            if let Some(inner) = self.parse_type() {
                return Some(Type::Mut(Box::new(inner), span));
            }
            return None;
        }

        if let Some(TokenKind::KwRef) = self.peek() {
            let t = self.advance().unwrap();
            let span = Span::from(&t);
            if let Some(inner) = self.parse_type() {
                return Some(Type::Ref(Box::new(inner), span));
            }
            return None;
        }

        let peeked = self.peek().cloned();
        match peeked {
            Some(TokenKind::KwVoid) => {
                let t = self.advance().unwrap();
                return Some(Type::Unit(Span::from(&t)));
            }
            Some(TokenKind::KwBool) => {
                let t = self.advance().unwrap();
                return Some(Type::Bool(Span::from(&t)));
            }
            Some(TokenKind::Ident(name)) => {
                let t = self.advance().unwrap();
                let span = Span::from(&t);
                let name = name.clone();

                if let Some(TokenKind::Lt) = self.peek() {
                    self.advance(); // <
                    let mut args = Vec::new();
                    loop {
                        self.skip_comments();
                        if let Some(ty) = self.parse_type() {
                            args.push(ty);
                        }
                        self.skip_comments();
                        if let Some(TokenKind::Gt) = self.peek() {
                            self.advance();
                            break;
                        } else if let Some(TokenKind::Comma) = self.peek() {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    return Some(Type::Generic(name, args, span));
                }
                return Some(Type::Named(name, span));
            }
            _ => None,
        }
    }

    fn parse_expr(&mut self) -> Option<Expr> {
        self.parse_or_expr()
    }

    fn parse_or_expr(&mut self) -> Option<Expr> {
        let mut left = self.parse_and_expr()?;
        loop {
            self.skip_comments();
            if let Some(TokenKind::OrOr) = self.peek() {
                let op_tok = self.advance().unwrap();
                if let Some(right) = self.parse_and_expr() {
                    let span = Span {
                        start: match &left {
                            Expr::Ident(_, s) | Expr::IntLiteral(_, s) => s.start,
                            _ => left.span().start,
                        },
                        end: right.span().end,
                        line: op_tok.line,
                        col: op_tok.col,
                    };
                    left = Expr::BinaryOp(Box::new(left), "||".to_string(), Box::new(right), span);
                }
            } else {
                break;
            }
        }
        Some(left)
    }

    fn parse_and_expr(&mut self) -> Option<Expr> {
        let mut left = self.parse_comparison_expr()?;
        loop {
            self.skip_comments();
            if let Some(TokenKind::AndAnd) = self.peek() {
                let op_tok = self.advance().unwrap();
                if let Some(right) = self.parse_comparison_expr() {
                    let span = Span {
                        start: match &left {
                            Expr::Ident(_, s) => s.start,
                            _ => left.span().start,
                        },
                        end: right.span().end,
                        line: op_tok.line,
                        col: op_tok.col,
                    };
                    left = Expr::BinaryOp(Box::new(left), "&&".to_string(), Box::new(right), span);
                }
            } else {
                break;
            }
        }
        Some(left)
    }

    fn parse_comparison_expr(&mut self) -> Option<Expr> {
        let mut left = self.parse_additive_expr()?;
        loop {
            self.skip_comments();
            let op = match self.peek() {
                Some(TokenKind::EqEq) => "==",
                Some(TokenKind::BangEq) => "!=",
                Some(TokenKind::Lt) => "<",
                Some(TokenKind::Gt) => ">",
                Some(TokenKind::LtEq) => "<=",
                Some(TokenKind::GtEq) => ">=",
                Some(TokenKind::Bang) => "!",
                Some(TokenKind::BangBang) => "++",
                Some(TokenKind::Plus) => "+",
                Some(TokenKind::Minus) => "-",
                Some(TokenKind::Star) => "*",
                Some(TokenKind::Slash) => "/",
                Some(TokenKind::Percent) => "%",
                Some(TokenKind::Eq)
                    if !matches!(self.peek_token().map(|t| &t.kind), Some(TokenKind::EqEq)) =>
                {
                    "="
                }
                _ => break,
            };
            let op_tok = self.advance().unwrap();
            if let Some(right) = self.parse_additive_expr() {
                let span = Span {
                    start: match &left {
                        Expr::Ident(_, s) => s.start,
                        _ => left.span().start,
                    },
                    end: right.span().end,
                    line: op_tok.line,
                    col: op_tok.col,
                };
                left = Expr::BinaryOp(Box::new(left), op.to_string(), Box::new(right), span);
            } else {
                break;
            }
        }
        Some(left)
    }

    fn parse_additive_expr(&mut self) -> Option<Expr> {
        let mut left = self.parse_multiplicative_expr()?;
        loop {
            self.skip_comments();
            let op = match self.peek() {
                Some(TokenKind::Plus) => "+",
                Some(TokenKind::Minus) => "-",
                _ => break,
            };
            let op_tok = self.advance().unwrap();
            if let Some(right) = self.parse_multiplicative_expr() {
                let span = Span {
                    start: left.span().start,
                    end: right.span().end,
                    line: op_tok.line,
                    col: op_tok.col,
                };
                left = Expr::BinaryOp(Box::new(left), op.to_string(), Box::new(right), span);
            } else {
                break;
            }
        }
        Some(left)
    }

    fn parse_multiplicative_expr(&mut self) -> Option<Expr> {
        let mut left = self.parse_unary_expr()?;
        loop {
            self.skip_comments();
            let op = match self.peek() {
                Some(TokenKind::Star) => "*",
                Some(TokenKind::Slash) => "/",
                Some(TokenKind::Percent) => "%",
                _ => break,
            };
            let op_tok = self.advance().unwrap();
            if let Some(right) = self.parse_unary_expr() {
                let span = Span {
                    start: left.span().start,
                    end: right.span().end,
                    line: op_tok.line,
                    col: op_tok.col,
                };
                left = Expr::BinaryOp(Box::new(left), op.to_string(), Box::new(right), span);
            } else {
                break;
            }
        }
        Some(left)
    }

    fn parse_unary_expr(&mut self) -> Option<Expr> {
        self.skip_comments();

        if let Some(TokenKind::KwTrust) = self.peek() {
            let t = self.advance().unwrap();
            let span = Span::from(&t);
            if let Some(expr) = self.parse_primary_expr() {
                return Some(Expr::Trust(Box::new(expr), span));
            }
        }

        if let Some(TokenKind::Bang) = self.peek() {
            let t = self.advance().unwrap();
            let span = Span::from(&t);
            if let Some(expr) = self.parse_unary_expr() {
                return Some(Expr::UnaryOp("!".to_string(), Box::new(expr), span));
            }
        }

        if let Some(TokenKind::Minus) = self.peek() {
            let t = self.advance().unwrap();
            let span = Span::from(&t);
            if let Some(expr) = self.parse_unary_expr() {
                return Some(Expr::UnaryOp("-".to_string(), Box::new(expr), span));
            }
        }

        self.parse_postfix_expr()
    }

    fn parse_postfix_expr(&mut self) -> Option<Expr> {
        let mut expr = self.parse_primary_expr()?;

        loop {
            self.skip_comments();
            match self.peek() {
                Some(TokenKind::Dot) => {
                    self.advance();
                    if let Some(TokenKind::Ident(name)) = self.peek().cloned() {
                        self.advance();
                        if let Some(TokenKind::LParen) = self.peek() {
                            self.advance(); // (
                            let args = self.parse_expr_list(TokenKind::RParen);
                            expr =
                                Expr::MethodCall(Box::new(expr.clone()), name, args, expr.span());
                        } else {
                            expr = Expr::FieldAccess(Box::new(expr.clone()), name, expr.span());
                        }
                    } else {
                        break;
                    }
                }
                Some(TokenKind::LParen) => {
                    self.advance();
                    let args = self.parse_expr_list(TokenKind::RParen);
                    expr = Expr::Call(Box::new(expr.clone()), args, expr.span());
                }
                Some(TokenKind::KwRef) if { true } => {
                    break;
                }
                _ => break,
            }
        }

        // Handle "as" casts
        loop {
            self.skip_comments();
            if let Some(TokenKind::Ident(name)) = self.peek().cloned() {
                if name == "as" {
                    self.advance();
                    if let Some(ty) = self.parse_type() {
                        let span = ty.span();
                        expr = Expr::Cast(Box::new(expr), ty, span);
                        continue;
                    }
                }
            }
            break;
        }

        // Handle ? propagation
        self.skip_comments();
        if let Some(TokenKind::Question) = self.peek() {
            let t = self.advance().unwrap();
            let span = Span::from(&t);
            expr = Expr::Propagate(Box::new(expr), span);
        }

        Some(expr)
    }

    fn parse_primary_expr(&mut self) -> Option<Expr> {
        self.skip_comments();

        let peeked = self.peek().cloned();
        match peeked {
            Some(TokenKind::Ident(name)) => {
                let t = self.advance().unwrap();
                let span = Span::from(&t);
                match name.as_str() {
                    "some" => {
                        if let Some(e) = self.parse_unary_expr() {
                            return Some(Expr::SomeVariant(Box::new(e), span));
                        }
                        return Some(Expr::Ident("some".to_string(), span));
                    }
                    "none" => return Some(Expr::NoneLiteral(span)),
                    "ok" => {
                        if let Some(e) = self.parse_unary_expr() {
                            return Some(Expr::OkVariant(Box::new(e), span));
                        }
                        return Some(Expr::Ident("ok".to_string(), span));
                    }
                    "err" => {
                        if let Some(e) = self.parse_unary_expr() {
                            return Some(Expr::ErrVariant(Box::new(e), span));
                        }
                        return Some(Expr::Ident("err".to_string(), span));
                    }
                    "return" => {
                        self.skip_comments();
                        if matches!(
                            self.peek(),
                            Some(TokenKind::Semi) | Some(TokenKind::RBrace) | None
                        ) {
                            return Some(Expr::Return(None, span));
                        }
                        if let Some(e) = self.parse_expr() {
                            return Some(Expr::Return(Some(Box::new(e)), span));
                        }
                        return Some(Expr::Return(None, span));
                    }
                    "if" => return self.parse_if_expr(),
                    "for" => return self.parse_for_expr(),
                    "match" => return self.parse_match_expr(),
                    "when" => return self.parse_when_expr(),
                    "deref" => {
                        if let Some(TokenKind::LParen) = self.peek() {
                            self.advance();
                            if let Some(e) = self.parse_expr() {
                                self.skip_comments();
                                if let Some(TokenKind::RParen) = self.peek() {
                                    self.advance();
                                }
                                return Some(Expr::Deref(Box::new(e), span));
                            }
                        }
                    }
                    "addr" => {
                        if let Some(TokenKind::LParen) = self.peek() {
                            self.advance();
                            if let Some(e) = self.parse_expr() {
                                self.skip_comments();
                                if let Some(TokenKind::RParen) = self.peek() {
                                    self.advance();
                                }
                                return Some(Expr::AddrOf(Box::new(e), span));
                            }
                        }
                    }
                    "true" => return Some(Expr::BoolLiteral(true, span)),
                    "false" => return Some(Expr::BoolLiteral(false, span)),
                    _ => {}
                }
                return Some(Expr::Ident(name, span));
            }
            Some(TokenKind::At) => {
                let t = self.advance().unwrap();
                let span = Span::from(&t);
                // @intrinsic
                if let Some(TokenKind::Ident(name)) = self.peek().cloned() {
                    self.advance();
                    // Could be a call: @intrinsic(args)
                    if let Some(TokenKind::LParen) = self.peek() {
                        self.advance();
                        let args = self.parse_expr_list(TokenKind::RParen);
                        return Some(Expr::Intrinsic(name, args, span));
                    }
                    return Some(Expr::Ident(format!("@{}", name), span));
                }
                return Some(Expr::Ident("@".to_string(), span));
            }

            Some(TokenKind::IntLiteral(lit)) => {
                let t = self.advance().unwrap();
                let span = Span::from(&t);
                return Some(Expr::IntLiteral(lit.clone(), span));
            }
            Some(TokenKind::FloatLiteral(lit)) => {
                let t = self.advance().unwrap();
                let span = Span::from(&t);
                return Some(Expr::FloatLiteral(lit.clone(), span));
            }
            Some(TokenKind::StringLiteral(lit)) => {
                let t = self.advance().unwrap();
                let span = Span::from(&t);
                return Some(Expr::StringLiteral(lit.clone(), span));
            }
            Some(TokenKind::BacktickString(lit)) => {
                let t = self.advance().unwrap();
                let span = Span::from(&t);
                return Some(Expr::StringLiteral(lit.clone(), span));
            }

            Some(TokenKind::LParen) => {
                let open_span = self.advance().unwrap();
                self.skip_comments();
                if let Some(e) = self.parse_expr() {
                    let e_span = e.span();
                    self.skip_comments();
                    if let Some(TokenKind::RParen) = self.peek() {
                        self.advance();
                    }
                    let combined = Span {
                        start: open_span.start,
                        end: e_span.end,
                        line: open_span.line,
                        col: open_span.col,
                    };
                    return Some(Expr::Grouped(Box::new(e), combined));
                }
            }

            Some(TokenKind::LBrace) => {
                return self.parse_block_expr();
            }

            Some(TokenKind::LBracket) => {
                self.advance();
                let elems = self.parse_expr_list(TokenKind::RBracket);
                let span = Span {
                    start: 0,
                    end: 0,
                    line: 0,
                    col: 0,
                };
                return Some(Expr::ArrayLit(elems, span));
            }

            _ => {}
        }

        None
    }

    fn parse_if_expr(&mut self) -> Option<Expr> {
        let start = self.peek_token()?.start;

        self.skip_comments();
        let cond = self.parse_expr()?;

        self.skip_comments();
        if let Some(TokenKind::LBrace) = self.peek() {
            let then_block = self.parse_block_expr()?;

            self.skip_comments();
            if let Some(TokenKind::KwElif) = self.peek() {
                self.advance(); // elif
                let elif = self.parse_if_expr()?;
                let span = Span {
                    start,
                    end: elif.span().end,
                    line: 0,
                    col: 0,
                };
                return Some(Expr::If(
                    Box::new(cond),
                    Box::new(then_block),
                    Some(Box::new(elif)),
                    span,
                ));
            } else if let Some(TokenKind::KwElse) = self.peek() {
                self.advance(); // else
                if let Some(TokenKind::KwIf) = self.peek() {
                    // else if -> elif
                    self.advance();
                    let elif = self.parse_if_expr()?;
                    let span = Span {
                        start,
                        end: elif.span().end,
                        line: 0,
                        col: 0,
                    };
                    return Some(Expr::If(
                        Box::new(cond),
                        Box::new(then_block),
                        Some(Box::new(elif)),
                        span,
                    ));
                } else {
                    let else_block = self.parse_block_expr()?;
                    let span = Span {
                        start,
                        end: else_block.span().end,
                        line: 0,
                        col: 0,
                    };
                    return Some(Expr::If(
                        Box::new(cond),
                        Box::new(then_block),
                        Some(Box::new(else_block)),
                        span,
                    ));
                }
            } else {
                let span = Span {
                    start,
                    end: then_block.span().end,
                    line: 0,
                    col: 0,
                };
                return Some(Expr::If(Box::new(cond), Box::new(then_block), None, span));
            }
        }
        None
    }

    fn parse_for_expr(&mut self) -> Option<Expr> {
        let start = self.peek_token()?.start;
        self.skip_comments();

        if let Some(TokenKind::LBrace) = self.peek() {
            let body = self.parse_block_expr()?;
            let span = Span {
                start,
                end: body.span().end,
                line: 0,
                col: 0,
            };
            return Some(Expr::ForLoop(
                Some(ForLoopKind::Infinite),
                Box::new(Expr::Block(vec![], body.span())),
                Box::new(body),
                span,
            ));
        }

        self.skip_comments();

        let saved_pos = self.pos;

        if let Some(TokenKind::Ident(_)) = self.peek() {
            let mut found_in = false;
            for i in 0..5 {
                if let Some(tk) = self.tokens.get(self.pos + i + 1) {
                    if tk.kind == TokenKind::KwIn {
                        found_in = true;
                        break;
                    }
                    if tk.kind == TokenKind::Semi {
                        break;
                    }
                }
            }

            if found_in {
                if let Some(TokenKind::Ident(var)) = self.peek() {
                    self.advance();
                    self.skip_comments();
                    if let Some(TokenKind::KwIn) = self.peek() {
                        self.advance();
                        if let Some(collection) = self.parse_expr() {
                            self.skip_comments();
                            if let Some(body) = self.parse_block_expr() {
                                let span = Span {
                                    start,
                                    end: body.span().end,
                                    line: 0,
                                    col: 0,
                                };
                                return Some(Expr::ForLoop(
                                    Some(ForLoopKind::ForIn),
                                    Box::new(collection),
                                    Box::new(body),
                                    span,
                                ));
                            }
                        }
                    }
                }
                self.pos = saved_pos;
            }
        }

        let has_paren = matches!(self.peek(), Some(TokenKind::LParen));
        if has_paren {
            self.advance();
        }

        let init = self.parse_expr();
        self.skip_comments();
        if let Some(TokenKind::Semi) = self.peek() {
            self.advance();
        }
        let cond = self.parse_expr();
        self.skip_comments();
        if let Some(TokenKind::Semi) = self.peek() {
            self.advance();
        }
        let step = self.parse_expr();
        self.skip_comments();
        if has_paren {
            if let Some(TokenKind::RParen) = self.peek() {
                self.advance();
            }
        }
        self.skip_comments();

        if let Some(body) = self.parse_block_expr() {
            let span = Span {
                start,
                end: body.span().end,
                line: 0,
                col: 0,
            };
            let header = if let (Some(i), Some(c), Some(s)) = (&init, &cond, &step) {
                Expr::BinaryOp(
                    Box::new(i.clone()),
                    ";".into(),
                    Box::new(Expr::BinaryOp(
                        Box::new(c.clone()),
                        ";".into(),
                        Box::new(s.clone()),
                        c.span(),
                    )),
                    i.span(),
                )
            } else {
                Expr::Block(vec![], body.span())
            };
            return Some(Expr::ForLoop(
                Some(ForLoopKind::CStyle),
                Box::new(header),
                Box::new(body),
                span,
            ));
        }

        None
    }

    fn parse_match_expr(&mut self) -> Option<Expr> {
        let start = self.peek_token()?.start;
        self.skip_comments();

        let expr = self.parse_expr()?;
        self.skip_comments();

        if let Some(TokenKind::LBrace) = self.peek() {
            self.advance(); // {
            let mut arms = Vec::new();

            loop {
                self.skip_comments();
                if let Some(TokenKind::RBrace) = self.peek() {
                    self.advance();
                    break;
                }

                let pattern = self.parse_pattern()?;
                self.skip_comments();

                if let Some(TokenKind::Arrow) = self.peek() {
                    self.advance();
                }

                self.skip_comments();
                if let Some(body) = self.parse_expr() {
                    let span = body.span();
                    arms.push(MatchArm {
                        pattern,
                        body: Box::new(body),
                        span,
                    });
                } else {
                    break;
                }
            }

            let end = self.peek_token().map(|t| t.end).unwrap_or(start + 1);
            Some(Expr::Match(
                Box::new(expr),
                arms,
                Span {
                    start,
                    end,
                    line: 0,
                    col: 0,
                },
            ))
        } else {
            None
        }
    }

    fn parse_pattern(&mut self) -> Option<Pattern> {
        self.skip_comments();

        match self.peek().cloned() {
            Some(TokenKind::Ident(name)) => {
                let t = self.advance().unwrap();
                let span = Span::from(&t);
                match name.as_str() {
                    "some" => {
                        if let Some(inner) = self.parse_pattern() {
                            return Some(Pattern::SomePat(Box::new(inner), span));
                        }
                        return Some(Pattern::Ident("some".to_string(), span));
                    }
                    "ok" => {
                        if let Some(inner) = self.parse_pattern() {
                            return Some(Pattern::OkPat(Box::new(inner), span));
                        }
                        return Some(Pattern::Ident("ok".to_string(), span));
                    }
                    "err" => {
                        if let Some(inner) = self.parse_pattern() {
                            return Some(Pattern::ErrPat(Box::new(inner), span));
                        }
                        return Some(Pattern::Ident("err".to_string(), span));
                    }
                    "else" => return Some(Pattern::ElsePat(span)),
                    "true" => return Some(Pattern::Ident("true".to_string(), span)),
                    "false" => return Some(Pattern::Ident("false".to_string(), span)),
                    "_" => return Some(Pattern::Discard(span)),
                    _ => {
                        self.skip_comments();
                        if let Some(TokenKind::LParen) = self.peek() {
                            self.advance();
                            let mut args = Vec::new();
                            loop {
                                self.skip_comments();
                                if let Some(TokenKind::RParen) = self.peek() {
                                    self.advance();
                                    break;
                                }
                                if let Some(p) = self.parse_pattern() {
                                    args.push(p);
                                }
                                self.skip_comments();
                                if let Some(TokenKind::Comma) = self.peek() {
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                            return Some(Pattern::ConstructPat(name, args, span));
                        }
                        Some(Pattern::Ident(name, span))
                    }
                }
            }
            Some(TokenKind::StringLiteral(s)) => {
                let t = self.advance().unwrap();
                let span = Span::from(&t);
                Some(Pattern::StringPat(s, span))
            }
            _ => None,
        }
    }

    fn parse_when_expr(&mut self) -> Option<Expr> {
        let start = self.peek_token()?.start;
        let platform = if let Some(TokenKind::Ident(name)) = self.peek().cloned() {
            self.advance();
            name
        } else {
            return None;
        };

        self.skip_comments();
        if let Some(body) = self.parse_block_expr() {
            let span = Span {
                start,
                end: body.span().end,
                line: 0,
                col: 0,
            };
            Some(Expr::WhenExpr(platform, Box::new(body), span))
        } else {
            None
        }
    }

    fn parse_block_expr(&mut self) -> Option<Expr> {
        self.skip_comments();
        if let Some(TokenKind::LBrace) = self.peek() {
            let start = self.advance().unwrap();
            let mut stmts = Vec::new();

            loop {
                self.skip_comments();
                if let Some(TokenKind::RBrace) = self.peek() {
                    self.advance();
                    break;
                }
                if let Some(s) = self.parse_stmt() {
                    stmts.push(s);
                } else {
                    if let Some(e) = self.parse_expr() {
                        let span = e.span();
                        stmts.push(Stmt::Expr(Box::new(e), span));
                        self.skip_comments();
                        if let Some(TokenKind::Semi) = self.peek() {
                            self.advance();
                        }
                    } else {
                        break;
                    }
                }
            }

            let end = self.peek_token().map(|t| t.end).unwrap_or(start.end);
            Some(Expr::Block(
                stmts,
                Span {
                    start: start.start,
                    end,
                    line: start.line,
                    col: start.col,
                },
            ))
        } else {
            None
        }
    }

    fn parse_expr_list(&mut self, _terminator: TokenKind) -> Vec<Expr> {
        let mut exprs = Vec::new();
        loop {
            self.skip_comments();
            if let Some(TokenKind::RParen) = self.peek() {
                self.advance();
                break;
            }
            if let Some(e) = self.parse_expr() {
                exprs.push(e);
            }
            self.skip_comments();
            if let Some(TokenKind::Comma) = self.peek() {
                self.advance();
            } else {
                if let Some(TokenKind::RParen) = self.peek() {
                    self.advance();
                }
                break;
            }
        }
        exprs
    }

    fn parse_stmt(&mut self) -> Option<Stmt> {
        self.skip_comments();

        let first = self.peek().cloned();
        match first {
            Some(TokenKind::KwVal) => {
                let t = self.advance().unwrap();
                let span = Span::from(&t);
                self.skip_comments();
                if let Some(TokenKind::Ident(name)) = self.peek().cloned() {
                    self.advance();
                    let _name_span = Span::from(self.peek_token().unwrap_or(&t));
                    self.skip_comments();

                    let mut ty = None;
                    if let Some(TokenKind::Colon) = self.peek() {
                        self.advance();
                        ty = self.parse_type();
                    }

                    self.skip_comments();
                    if let Some(TokenKind::Eq) = self.peek() {
                        self.advance();
                    }
                    self.skip_comments();

                    if let Some(e) = self.parse_expr() {
                        self.skip_comments();
                        if let Some(TokenKind::Semi) = self.peek() {
                            self.advance();
                        }
                        return Some(Stmt::Let(name, ty, Box::new(e), span, false));
                    }
                }
                None
            }
            Some(TokenKind::Ident(ref name_str)) if name_str == "use" => {
                let _name = self.advance().unwrap();
                let _span = Span::from(&_name);
                self.skip_comments();
                if let Some(TokenKind::LParen) = self.peek() {
                    self.advance();
                    self.skip_comments();
                    let mod_path = if let Some(TokenKind::StringLiteral(p)) = self.peek().cloned() {
                        self.advance();
                        p
                    } else {
                        String::new()
                    };
                    self.skip_comments();
                    if let Some(TokenKind::RParen) = self.peek() {
                        self.advance();
                    }
                    self.skip_comments();
                    if let Some(TokenKind::Semi) = self.peek() {
                        self.advance();
                    }
                    return Some(Stmt::Use("use".to_string(), mod_path, _span));
                }
                None
            }
            Some(TokenKind::KwGuarded) | Some(TokenKind::KwPre) | Some(TokenKind::KwPost) => None,
            Some(TokenKind::KwReturn) => {
                let t = self.advance().unwrap();
                let span = Span::from(&t);
                self.skip_comments();
                let expr = self.parse_expr();
                self.skip_comments();
                if let Some(TokenKind::Semi) = self.peek() {
                    self.advance();
                }
                let span2 = span.clone();
                return Some(Stmt::Expr(
                    Box::new(Expr::Return(expr.map(Box::new), span)),
                    span2,
                ));
            }
            Some(TokenKind::KwMatch) => {
                let t = self.advance().unwrap();
                let span = Span::from(&t);
                if let Some(e) = self.parse_match_expr() {
                    self.skip_comments();
                    if let Some(TokenKind::Semi) = self.peek() {
                        self.advance();
                    }
                    return Some(Stmt::Expr(Box::new(e), span));
                }
                None
            }
            Some(TokenKind::KwIf) => {
                let t = self.advance().unwrap();
                let span = Span::from(&t);
                if let Some(e) = self.parse_if_expr() {
                    self.skip_comments();
                    if let Some(TokenKind::Semi) = self.peek() {
                        self.advance();
                    }
                    return Some(Stmt::Expr(Box::new(e), span));
                }
                None
            }
            Some(TokenKind::KwFor) => {
                let t = self.advance().unwrap();
                let span = Span::from(&t);
                if let Some(e) = self.parse_for_expr() {
                    self.skip_comments();
                    if let Some(TokenKind::Semi) = self.peek() {
                        self.advance();
                    }
                    return Some(Stmt::Expr(Box::new(e), span));
                }
                None
            }
            Some(TokenKind::Ident(name)) if name == "defer" => {
                let t = self.advance().unwrap();
                let span = Span::from(&t);
                self.skip_comments();
                if let Some(e) = self.parse_block_expr() {
                    self.skip_comments();
                    if let Some(TokenKind::Semi) = self.peek() {
                        self.advance();
                    }
                    return Some(Stmt::Expr(Box::new(e), span));
                }
                None
            }
            Some(TokenKind::KwAssert) => {
                let t = self.advance().unwrap();
                let span = Span::from(&t);
                self.skip_comments();
                if let Some(e) = self.parse_expr() {
                    self.skip_comments();
                    if let Some(TokenKind::Semi) = self.peek() {
                        self.advance();
                    }
                    return Some(Stmt::Expr(Box::new(e), span));
                }
                None
            }
            _ => {
                if let Some(e) = self.parse_expr() {
                    let span = e.span();
                    self.skip_comments();
                    if let Some(TokenKind::Eq) = self.peek() {
                        self.advance();
                        self.skip_comments();
                        if let Some(rhs) = self.parse_expr() {
                            self.skip_comments();
                            if let Some(TokenKind::Semi) = self.peek() {
                                self.advance();
                            }
                            return Some(Stmt::Assign(Box::new(e), Box::new(rhs), span));
                        }
                    }
                    self.skip_comments();
                    if let Some(TokenKind::Semi) = self.peek() {
                        self.advance();
                    }
                    return Some(Stmt::Expr(Box::new(e), span));
                }
                None
            }
        }
    }

    fn parse_params(&mut self) -> Vec<Param> {
        let mut params = Vec::new();
        loop {
            self.skip_comments();
            if let Some(TokenKind::RParen) = self.peek() {
                break;
            }
            self.skip_comments();
            if let Some(TokenKind::Ident(name)) = self.peek().cloned() {
                let name_tok = self.advance().unwrap();
                let name_span = Span::from(&name_tok);
                self.skip_comments();
                if let Some(TokenKind::Colon) = self.peek() {
                    self.advance();
                    if let Some(ty) = self.parse_type() {
                        let span = Span {
                            start: name_span.start,
                            end: ty.span().end,
                            line: name_span.line,
                            col: name_span.col,
                        };
                        params.push(Param {
                            name,
                            name_span: name_span,
                            ty,
                            span,
                        });
                    }
                }
            }
            self.skip_comments();
            if let Some(TokenKind::Comma) = self.peek() {
                self.advance();
            } else {
                break;
            }
        }
        self.skip_comments();
        if let Some(TokenKind::RParen) = self.peek() {
            self.advance();
        }
        params
    }

    fn parse_fn(&mut self, is_pub: bool, doc_comments: Vec<String>) -> Option<Item> {
        let start = self.peek_token()?.start;

        let mut self_type = None;
        let mut self_mut = false;

        self.skip_comments();
        if let Some(TokenKind::LParen) = self.peek() {
            self.advance();
            if let Some(TokenKind::Ident(ty_name)) = self.peek().cloned() {
                self.advance();
                self.skip_comments();
                if let Some(TokenKind::KwMut) = self.peek() {
                    self_mut = true;
                    self.advance();
                }
                self.skip_comments();
                if let Some(TokenKind::KwSelf) = self.peek() {
                    self.advance();
                }
                self_type = Some(ty_name);
            }
            self.skip_comments();
            if let Some(TokenKind::RParen) = self.peek() {
                self.advance();
            }
        }

        self.skip_comments();
        let name = if let Some(TokenKind::Ident(n)) = self.peek().cloned() {
            self.advance();
            n
        } else {
            return None;
        };
        let name_span = Span::from(self.peek_token().unwrap_or(&Token {
            kind: TokenKind::Unknown(' '),
            start: 0,
            end: 0,
            line: 0,
            col: 0,
        }));

        self.skip_comments();
        let params = if let Some(TokenKind::LParen) = self.peek() {
            self.advance();
            self.parse_params()
        } else {
            Vec::new()
        };

        self.skip_comments();
        if let Some(TokenKind::Arrow) = self.peek() {
            self.advance(); // ->
        }

        self.skip_comments();
        let return_type = self.parse_type().unwrap_or(Type::Unit(Span {
            start: 0,
            end: 0,
            line: 0,
            col: 0,
        }));
        let returns_untrusted = {
            self.skip_comments();
            if let Some(TokenKind::Bang) = self.peek() {
                self.advance();
                true
            } else {
                false
            }
        };

        self.skip_comments();
        if let Some(TokenKind::Eq) = self.peek() {
            self.advance();
        }

        let mut pre = None;
        let mut post = None;
        let mut body = None;

        self.skip_comments();
        loop {
            if let Some(TokenKind::LBrace) = self.peek() {
                body = self.parse_block_expr();
                break;
            } else if let Some(TokenKind::KwGuarded) = self.peek() {
                self.advance();
                self.skip_comments();
                if let Some(TokenKind::KwPre) = self.peek() {
                    pre = self.parse_condition_block(true);
                }
            } else if let Some(TokenKind::KwPre) = self.peek() {
                pre = self.parse_condition_block(false);
            } else if let Some(TokenKind::KwPost) = self.peek() {
                post = self.parse_condition_block(false);
            } else {
                break;
            }
        }

        let end = body.as_ref().map(|e| e.span().end).unwrap_or(start + 1);
        let span = Span {
            start,
            end,
            line: 0,
            col: 0,
        };

        Some(Item::Function(Function {
            name,
            name_span,
            self_type,
            self_mut,
            params,
            return_type,
            returns_untrusted,
            pre,
            post,
            body,
            is_pub,
            is_extern: false,
            extern_abi: None,
            extern_name: None,
            span,
            doc_comments,
        }))
    }

    fn parse_condition_block(&mut self, guarded: bool) -> Option<ConditionBlock> {
        let start = self.peek_token()?.start;
        self.skip_comments();
        if let Some(TokenKind::LBrace) = self.peek() {
            self.advance();
        }
        let mut conditions = Vec::new();

        loop {
            self.skip_comments();
            if let Some(TokenKind::RBrace) = self.peek() {
                self.advance();
                break;
            }

            if let Some(TokenKind::Ident(name)) = self.peek().cloned() {
                let t = self.advance().unwrap();
                let name_span = Span::from(&t);
                self.skip_comments();
                if let Some(TokenKind::Colon) = self.peek() {
                    self.advance();
                }
                self.skip_comments();
                if let Some(e) = self.parse_expr() {
                    conditions.push(Condition {
                        name,
                        name_span,
                        expr: Box::new(e),
                    });
                }
            }

            self.skip_comments();
        }

        let end = self.peek_token().map(|t| t.end).unwrap_or(start);
        Some(ConditionBlock {
            conditions,
            guarded,
            span: Span {
                start,
                end,
                line: 0,
                col: 0,
            },
        })
    }

    fn parse_extern_fn(&mut self, is_pub: bool, doc_comments: Vec<String>) -> Option<Item> {
        let start = self.peek_token()?.start;

        self.skip_comments();
        let mut abi = None;
        if let Some(TokenKind::LParen) = self.peek() {
            self.advance();
            if let Some(TokenKind::Ident(a)) = self.peek().cloned() {
                abi = Some(a);
                self.advance();
            }
            if let Some(TokenKind::RParen) = self.peek() {
                self.advance();
            }
        }

        self.skip_comments();
        if let Some(TokenKind::KwFn) = self.peek() {
            self.advance();
        }

        self.skip_comments();
        let name = if let Some(TokenKind::Ident(n)) = self.peek().cloned() {
            self.advance();
            n
        } else {
            return None;
        };

        self.skip_comments();
        let params = if let Some(TokenKind::LParen) = self.peek() {
            self.advance();
            self.parse_params()
        } else {
            Vec::new()
        };

        self.skip_comments();
        if let Some(TokenKind::Arrow) = self.peek() {
            self.advance();
        }

        self.skip_comments();
        let return_type = self.parse_type().unwrap_or(Type::Unit(Span {
            start: 0,
            end: 0,
            line: 0,
            col: 0,
        }));
        let returns_untrusted = {
            self.skip_comments();
            if let Some(TokenKind::Bang) = self.peek() {
                self.advance();
                true
            } else {
                true // externs are always untrusted
            }
        };

        self.skip_comments();
        let mut extern_name = None;
        if let Some(TokenKind::Eq) = self.peek() {
            self.advance();
            self.skip_comments();
            if let Some(TokenKind::StringLiteral(ename)) = self.peek().cloned() {
                extern_name = Some(ename);
                self.advance();
            }
        }

        let end = self.peek_token().map(|t| t.end).unwrap_or(start + 1);
        let span = Span {
            start,
            end,
            line: 0,
            col: 0,
        };

        Some(Item::Function(Function {
            name,
            name_span: span.clone(),
            self_type: None,
            self_mut: false,
            params,
            return_type,
            returns_untrusted,
            pre: None,
            post: None,
            body: None,
            is_pub,
            is_extern: true,
            extern_abi: abi,
            extern_name,
            span,
            doc_comments,
        }))
    }

    fn parse_extern_type(&mut self, is_pub: bool, doc_comments: Vec<String>) -> Option<Item> {
        let start = self.peek_token()?.start;

        self.skip_comments();
        if let Some(TokenKind::KwType) = self.peek() {
            self.advance();
        }

        self.skip_comments();
        let name = if let Some(TokenKind::Ident(n)) = self.peek().cloned() {
            self.advance();
            n
        } else {
            return None;
        };
        let name_span = Span::from(self.peek_token().unwrap_or(&Token {
            kind: TokenKind::Unknown(' '),
            start,
            end: start,
            line: 0,
            col: 0,
        }));

        self.skip_comments();
        let fields = if let Some(TokenKind::LBrace) = self.peek() {
            self.parse_struct_fields()
        } else {
            Vec::new()
        };

        let end = self.peek_token().map(|t| t.end).unwrap_or(start + 1);
        let span = Span {
            start,
            end,
            line: 0,
            col: 0,
        };

        Some(Item::TypeDef(TypeDef {
            name,
            name_span,
            kind: TypeDefKind::Struct(fields),
            is_pub,
            is_extern: true,
            span,
            doc_comments,
        }))
    }

    fn parse_type_def(&mut self, is_pub: bool, doc_comments: Vec<String>) -> Option<Item> {
        let start = self.peek_token()?.start;

        self.skip_comments();
        let name = if let Some(TokenKind::Ident(n)) = self.peek().cloned() {
            self.advance();
            n
        } else {
            return None;
        };
        let name_span = Span::from(self.peek_token().unwrap_or(&Token {
            kind: TokenKind::Unknown(' '),
            start,
            end: start,
            line: 0,
            col: 0,
        }));

        self.skip_comments();
        if let Some(TokenKind::Eq) = self.peek() {
            self.advance();
        }

        self.skip_comments();
        let kind = if let Some(TokenKind::LBrace) = self.peek() {
            let fields = self.parse_struct_fields();
            if fields.is_empty() {
                let variants = self.parse_enum_variants();
                if !variants.is_empty() {
                    TypeDefKind::Enum(variants)
                } else {
                    TypeDefKind::Struct(vec![])
                }
            } else {
                TypeDefKind::Struct(fields)
            }
        } else {
            return None;
        };

        let end = self.peek_token().map(|t| t.end).unwrap_or(start + 1);
        let span = Span {
            start,
            end,
            line: 0,
            col: 0,
        };

        Some(Item::TypeDef(TypeDef {
            name,
            name_span,
            kind,
            is_pub,
            is_extern: false,
            span,
            doc_comments,
        }))
    }

    fn parse_union_def(&mut self, is_pub: bool, doc_comments: Vec<String>) -> Option<Item> {
        let start = self.peek_token()?.start;

        self.skip_comments();
        let name = if let Some(TokenKind::Ident(n)) = self.peek().cloned() {
            self.advance();
            n
        } else {
            return None;
        };
        let name_span = Span::from(self.peek_token().unwrap_or(&Token {
            kind: TokenKind::Unknown(' '),
            start,
            end: start,
            line: 0,
            col: 0,
        }));

        self.skip_comments();
        if let Some(TokenKind::Eq) = self.peek() {
            self.advance();
        }

        self.skip_comments();
        if let Some(TokenKind::LBrace) = self.peek() {
            self.advance(); // {
            self.skip_comments();

            let first_is_construct = if let Some(TokenKind::Ident(_)) = self.peek() {
                let saved = self.pos;
                self.advance();
                let result = matches!(self.peek(), Some(TokenKind::LParen));
                self.pos = saved;
                result
            } else {
                false
            };

            let kind = if first_is_construct {
                TypeDefKind::UnionConstruct(self.parse_union_variants())
            } else {
                TypeDefKind::UnionConstruct(self.parse_union_variants())
            };

            self.skip_comments();
            if let Some(TokenKind::RBrace) = self.peek() {
                self.advance();
            }

            let end = self.peek_token().map(|t| t.end).unwrap_or(start + 1);
            let span = Span {
                start,
                end,
                line: 0,
                col: 0,
            };

            return Some(Item::TypeDef(TypeDef {
                name,
                name_span,
                kind,
                is_pub,
                is_extern: false,
                span,
                doc_comments,
            }));
        }

        None
    }

    fn parse_enum_def(&mut self, is_pub: bool, doc_comments: Vec<String>) -> Option<Item> {
        let start = self.peek_token()?.start;

        self.skip_comments();
        let name = if let Some(TokenKind::Ident(n)) = self.peek().cloned() {
            self.advance();
            n
        } else {
            return None;
        };
        let name_span = Span::from(self.peek_token().unwrap_or(&Token {
            kind: TokenKind::Unknown(' '),
            start,
            end: start,
            line: 0,
            col: 0,
        }));

        self.skip_comments();
        if let Some(TokenKind::Eq) = self.peek() {
            self.advance();
        }

        self.skip_comments();
        let variants = if let Some(TokenKind::LBrace) = self.peek() {
            self.advance();
            self.parse_enum_variants()
        } else {
            Vec::new()
        };

        let end = self.peek_token().map(|t| t.end).unwrap_or(start + 1);
        let span = Span {
            start,
            end,
            line: 0,
            col: 0,
        };

        Some(Item::TypeDef(TypeDef {
            name,
            name_span,
            kind: TypeDefKind::Enum(variants),
            is_pub,
            is_extern: false,
            span,
            doc_comments,
        }))
    }

    fn parse_struct_fields(&mut self) -> Vec<StructField> {
        let mut fields = Vec::new();
        if let Some(TokenKind::LBrace) = self.peek() {
            self.advance();
        }

        loop {
            self.skip_comments();
            if let Some(TokenKind::RBrace) = self.peek() {
                self.advance();
                break;
            }
            if let Some(TokenKind::Ident(name)) = self.peek().cloned() {
                let name_tok = self.advance().unwrap();
                let name_span = Span::from(&name_tok);
                self.skip_comments();
                let is_mut = if let Some(TokenKind::KwMut) = self.peek() {
                    self.advance();
                    true
                } else {
                    false
                };
                self.skip_comments();
                if let Some(TokenKind::Colon) = self.peek() {
                    self.advance();
                }
                self.skip_comments();
                if let Some(ty) = self.parse_type() {
                    let span = Span {
                        start: name_span.start,
                        end: ty.span().end,
                        line: name_span.line,
                        col: name_span.col,
                    };
                    fields.push(StructField {
                        name,
                        name_span,
                        ty,
                        is_mut,
                        span,
                    });
                }
            }
            self.skip_comments();
            if let Some(TokenKind::Comma) = self.peek() {
                self.advance();
            }
        }
        fields
    }

    fn parse_union_variants(&mut self) -> Vec<UnionVariant> {
        let mut variants = Vec::new();
        loop {
            self.skip_comments();
            if let Some(TokenKind::RBrace) = self.peek() {
                self.advance();
                break;
            }

            if let Some(TokenKind::Ident(name)) = self.peek().cloned() {
                let t = self.advance().unwrap();
                let name_span = Span::from(&t);

                self.skip_comments();
                if let Some(TokenKind::LParen) = self.peek() {
                    self.advance();
                    if let Some(ty) = self.parse_type() {
                        self.skip_comments();
                        if let Some(TokenKind::RParen) = self.peek() {
                            self.advance();
                        }
                        let span = Span {
                            start: name_span.start,
                            end: ty.span().end,
                            line: name_span.line,
                            col: name_span.col,
                        };
                        variants.push(UnionVariant {
                            name,
                            name_span,
                            ty,
                            span,
                        });
                    }
                } else if let Some(TokenKind::RBrace) | Some(TokenKind::Unknown('|')) = self.peek()
                {
                    let span = name_span.clone();
                    variants.push(UnionVariant {
                        name,
                        name_span,
                        ty: Type::Unit(Span {
                            start: 0,
                            end: 0,
                            line: 0,
                            col: 0,
                        }),
                        span,
                    });
                }
            }

            self.skip_comments();
            if let Some(TokenKind::Unknown('|')) | Some(TokenKind::OrOr) = self.peek() {
                self.advance();
            }
            if let Some(TokenKind::Comma) = self.peek() {
                self.advance();
            }

            if variants.len() > 100 {
                break;
            }
        }
        variants
    }

    fn parse_enum_variants(&mut self) -> Vec<EnumVariant> {
        let mut variants = Vec::new();
        loop {
            self.skip_comments();
            if let Some(TokenKind::RBrace) = self.peek() {
                self.advance();
                break;
            }
            if let Some(TokenKind::Ident(name)) = self.peek().cloned() {
                let t = self.advance().unwrap();
                let name_span = Span::from(&t);
                variants.push(EnumVariant {
                    name,
                    name_span: name_span.clone(),
                    span: name_span.clone(),
                });
            }
            self.skip_comments();
            if let Some(TokenKind::Comma) = self.peek() {
                self.advance();
            }
            if variants.len() > 100 {
                break;
            }
        }
        variants
    }

    fn parse_use_stmt(&mut self) -> Option<Item> {
        if let Some(TokenKind::Ident(name)) = self.peek().cloned() {
            if name == "use" {
                let t = self.advance().unwrap();
                let span = Span::from(&t);
                self.skip_comments();
                if let Some(TokenKind::LParen) = self.peek() {
                    self.advance();
                    self.skip_comments();
                    let mod_path = if let Some(TokenKind::StringLiteral(p)) = self.peek().cloned() {
                        self.advance();
                        p
                    } else {
                        String::new()
                    };
                    self.skip_comments();
                    if let Some(TokenKind::RParen) = self.peek() {
                        self.advance();
                    }
                    self.skip_comments();
                    if let Some(TokenKind::Semi) = self.peek() {
                        self.advance();
                    }
                    return Some(Item::Use(name.clone(), mod_path, span));
                }
            }
        }
        None
    }

    fn parse_test_block(&mut self) -> Option<Item> {
        let start = self.peek_token()?.start;
        self.skip_comments();
        if let Some(body) = self.parse_block_expr() {
            let span = Span {
                start,
                end: body.span().end,
                line: 0,
                col: 0,
            };
            Some(Item::Test(TestBlock { body, span }))
        } else {
            None
        }
    }

    pub fn parse_module(&mut self) -> Module {
        let mut items = Vec::new();

        loop {
            self.skip_comments();

            if self.pos >= self.tokens.len() {
                break;
            }

            let mut docs = Vec::new();
            while let Some(t) = self.peek_token() {
                if t.kind == TokenKind::Comment {
                    self.advance();
                } else if let TokenKind::DocComment(text) = &t.kind {
                    docs.push(text.clone());
                    self.advance();
                } else {
                    break;
                }
            }

            self.skip_comments();
            if self.pos >= self.tokens.len() {
                break;
            }

            let is_pub = if let Some(TokenKind::KwPub) = self.peek() {
                self.advance();
                true
            } else {
                false
            };

            self.skip_comments();

            match self.peek().cloned() {
                Some(TokenKind::KwFn) => {
                    self.advance();
                    if let Some(item) = self.parse_fn(is_pub, docs) {
                        items.push(item);
                    }
                }
                Some(TokenKind::KwType) => {
                    self.advance();
                    if let Some(item) = self.parse_type_def(is_pub, docs) {
                        items.push(item);
                    }
                }
                Some(TokenKind::KwUnion) => {
                    self.advance();
                    if let Some(item) = self.parse_union_def(is_pub, docs) {
                        items.push(item);
                    }
                }
                Some(TokenKind::KwEnum) => {
                    self.advance();
                    if let Some(item) = self.parse_enum_def(is_pub, docs) {
                        items.push(item);
                    }
                }
                Some(TokenKind::KwExtern) => {
                    self.advance();
                    self.skip_comments();
                    if let Some(TokenKind::KwType) = self.peek() {
                        self.advance();
                        if let Some(item) = self.parse_extern_type(is_pub, docs) {
                            items.push(item);
                        }
                    } else if let Some(TokenKind::LParen) = self.peek() {
                        if let Some(item) = self.parse_extern_fn(is_pub, docs) {
                            items.push(item);
                        }
                    } else if let Some(TokenKind::KwFn) = self.peek() {
                        if let Some(item) = self.parse_extern_fn(is_pub, docs) {
                            items.push(item);
                        }
                    }
                }
                Some(TokenKind::KwTest) => {
                    self.advance();
                    if let Some(item) = self.parse_test_block() {
                        items.push(item);
                    }
                }
                Some(TokenKind::Ident(name)) if name == "use" => {
                    if let Some(item) = self.parse_use_stmt() {
                        items.push(item);
                    }
                }
                Some(TokenKind::KwVal) => {
                    self.advance();
                    self.skip_comments();
                    if let Some(TokenKind::Ident(var_name)) = self.peek().cloned() {
                        let name_tok = self.advance().unwrap();
                        let span = Span::from(&name_tok);
                        self.skip_comments();
                        let mut ty = None;
                        if let Some(TokenKind::Colon) = self.peek() {
                            self.advance();
                            ty = self.parse_type();
                        }
                        self.skip_comments();
                        if let Some(TokenKind::Eq) = self.peek() {
                            self.advance();
                        }
                        self.skip_comments();
                        if let Some(e) = self.parse_expr() {
                            self.skip_comments();
                            if let Some(TokenKind::Semi) = self.peek() {
                                self.advance();
                            }
                            items.push(Item::Function(Function {
                                name: var_name.clone(),
                                name_span: Span::from(&name_tok),
                                self_type: None,
                                self_mut: false,
                                params: vec![],
                                return_type: ty.unwrap_or(Type::Unit(Span {
                                    start: 0,
                                    end: 0,
                                    line: 0,
                                    col: 0,
                                })),
                                returns_untrusted: false,
                                pre: None,
                                post: None,
                                body: Some(e),
                                is_pub,
                                is_extern: false,
                                extern_abi: None,
                                extern_name: None,
                                span,
                                doc_comments: docs,
                            }));
                        }
                    }
                }
                Some(TokenKind::KwWhen) => {
                    self.advance();
                    self.skip_comments();
                    if let Some(when_expr) = self.parse_when_expr() {
                        items.push(Item::Function(Function {
                            name: format!("when_block_{}", self.pos),
                            name_span: Span {
                                start: 0,
                                end: 0,
                                line: 0,
                                col: 0,
                            },
                            self_type: None,
                            self_mut: false,
                            params: vec![],
                            return_type: Type::Unit(Span {
                                start: 0,
                                end: 0,
                                line: 0,
                                col: 0,
                            }),
                            returns_untrusted: false,
                            pre: None,
                            post: None,
                            body: Some(when_expr),
                            is_pub,
                            is_extern: false,
                            extern_abi: None,
                            extern_name: None,
                            span: Span {
                                start: 0,
                                end: 0,
                                line: 0,
                                col: 0,
                            },
                            doc_comments: docs,
                        }));
                    }
                }
                _ => {
                    self.advance();
                }
            }
        }

        Module {
            items,
            source: self.source.clone(),
        }
    }
}

fn span_from_tokens<'a>(spans: impl IntoIterator<Item = &'a Span>) -> Span {
    let mut iter = spans.into_iter();
    let first = iter.next().cloned();
    let mut last = first.clone();
    for s in iter {
        last = Some(s.clone());
    }
    match (first, last) {
        (Some(f), Some(l)) => Span {
            start: f.start,
            end: l.end,
            line: f.line,
            col: f.col,
        },
        _ => Span {
            start: 0,
            end: 0,
            line: 0,
            col: 0,
        },
    }
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Ident(_, s) => s.clone(),
            Expr::IntLiteral(_, s) => s.clone(),
            Expr::FloatLiteral(_, s) => s.clone(),
            Expr::StringLiteral(_, s) => s.clone(),
            Expr::BoolLiteral(_, s) => s.clone(),
            Expr::NoneLiteral(s) => s.clone(),
            Expr::BinaryOp(_, _, _, s) => s.clone(),
            Expr::UnaryOp(_, _, s) => s.clone(),
            Expr::Call(_, _, s) => s.clone(),
            Expr::MethodCall(_, _, _, s) => s.clone(),
            Expr::FieldAccess(_, _, s) => s.clone(),
            Expr::Index(_, _, s) => s.clone(),
            Expr::Cast(_, _, s) => s.clone(),
            Expr::Trust(_, s) => s.clone(),
            Expr::Return(_, s) => s.clone(),
            Expr::If(_, _, _, s) => s.clone(),
            Expr::ForLoop(_, _, _, s) => s.clone(),
            Expr::Match(_, _, s) => s.clone(),
            Expr::Block(_, s) => s.clone(),
            Expr::StructLit(_, s) => s.clone(),
            Expr::ArrayLit(_, s) => s.clone(),
            Expr::SomeVariant(_, s) => s.clone(),
            Expr::OkVariant(_, s) => s.clone(),
            Expr::ErrVariant(_, s) => s.clone(),
            Expr::WhenExpr(_, _, s) => s.clone(),
            Expr::Deref(_, s) => s.clone(),
            Expr::AddrOf(_, s) => s.clone(),
            Expr::Propagate(_, s) => s.clone(),
            Expr::Grouped(_, s) => s.clone(),
            Expr::Intrinsic(_, _, s) => s.clone(),
        }
    }
}
