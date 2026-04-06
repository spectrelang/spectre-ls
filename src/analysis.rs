use crate::ast::*;
use crate::lexer::Lexer;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub span: Span,
    pub type_str: Option<String>,
    pub doc: String,
}

#[derive(Debug, Clone)]
pub enum SymbolKind {
    Function,
    Type,
    Field,
    Variable,
    Parameter,
    EnumVariant,
    UnionVariant,
    Module,
    Constant,
}

#[derive(Debug, Clone)]
pub struct DocumentAnalysis {
    pub module: Module,
    pub symbols: Vec<SymbolInfo>,
    pub ident_spans: Vec<(Span, IdentContext)>,
    pub fn_by_name: HashMap<String, Function>,
    pub type_defs: HashMap<String, TypeDef>,
    pub var_scopes: Vec<Scope>,
    pub symbol_at: HashMap<usize, SymbolInfo>,
    pub resolves_to: HashMap<Span, Span>,
}

#[derive(Debug, Clone)]
pub struct Scope {
    pub start: usize,
    pub end: usize,
    pub variables: HashMap<String, (String, Span)>, // name -> (type, def_span)
}

#[derive(Debug, Clone)]
pub enum IdentContext {
    FunctionCall,
    FunctionDef,
    TypeRef,
    VariableDef,
    VariableRef,
    Parameter,
    FieldAccess,
    FieldDef,
    MethodCall(String),
    ModuleAccess,
    Keyword,
    EnumVariant,
    UnionVariant,
}

pub fn analyze(source: &str) -> DocumentAnalysis {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens, source.to_string());
    let module = parser.parse_module();

    let mut symbols: Vec<SymbolInfo> = Vec::new();
    let mut fn_by_name: HashMap<String, Function> = HashMap::new();
    let mut type_defs: HashMap<String, TypeDef> = HashMap::new();
    let mut ident_spans: Vec<(Span, IdentContext)> = Vec::new();
    let mut symbol_at: HashMap<usize, SymbolInfo> = HashMap::new();
    let mut resolves_to: HashMap<Span, Span> = HashMap::new();

    for item in &module.items {
        match item {
            Item::Function(f) => {
                let mut doc = f.doc_comments.join("\n");
                if let Some(pre) = &f.pre {
                    if !pre.conditions.is_empty() {
                        if !doc.is_empty() {
                            doc.push_str("\n\n");
                        }
                        doc.push_str("**Preconditions:**\n");
                        for cond in &pre.conditions {
                            if pre.guarded {
                                doc.push_str(&format!(
                                    "- `{}` (guarded): `{}`\n",
                                    cond.name,
                                    expr_source(source, &cond.expr)
                                ));
                            } else {
                                doc.push_str(&format!(
                                    "- `{}`: `{}`\n",
                                    cond.name,
                                    expr_source(source, &cond.expr)
                                ));
                            }
                        }
                    }
                }
                if let Some(post) = &f.post {
                    if !post.conditions.is_empty() {
                        if !doc.is_empty() {
                            doc.push_str("\n\n");
                        }
                        doc.push_str("**Postconditions:**\n");
                        for cond in &post.conditions {
                            doc.push_str(&format!(
                                "- `{}`: `{}`\n",
                                cond.name,
                                expr_source(source, &cond.expr)
                            ));
                        }
                    }
                }

                let ret_display = if f.returns_untrusted {
                    format!("{}!", f.return_type.display())
                } else {
                    f.return_type.display()
                };

                let sig = format!(
                    "fn {}({}) -> {}",
                    f.name,
                    f.params
                        .iter()
                        .map(|p| format!("{}: {}", p.name, p.ty.display()))
                        .collect::<Vec<_>>()
                        .join(", "),
                    ret_display
                );

                let sym = SymbolInfo {
                    name: f.name.clone(),
                    kind: if f.params.is_empty() && f.body.is_some() && f.self_type.is_none() {
                        SymbolKind::Constant
                    } else {
                        SymbolKind::Function
                    },
                    span: f.span.clone(),
                    type_str: Some(sig),
                    doc,
                };
                symbols.push(sym.clone());

                for offset in f.span.range() {
                    symbol_at.insert(offset, sym.clone());
                }

                fn_by_name.insert(f.name.clone(), f.clone());

                for param in &f.params {
                    ident_spans.push((param.name_span.clone(), IdentContext::Parameter));
                }

                ident_spans.push((f.name_span.clone(), IdentContext::FunctionDef));
            }
            Item::TypeDef(td) => {
                let kind_str = match &td.kind {
                    TypeDefKind::Struct(_) => "struct",
                    TypeDefKind::Union(_) => "union",
                    TypeDefKind::UnionConstruct(_) => "union (constructors)",
                    TypeDefKind::Enum(_) => "enum",
                };

                let doc = td.doc_comments.join("\n");
                let sym = SymbolInfo {
                    name: td.name.clone(),
                    kind: SymbolKind::Type,
                    span: td.span.clone(),
                    type_str: Some(format!("{} {}", kind_str, td.name)),
                    doc,
                };
                symbols.push(sym.clone());

                for offset in td.span.range() {
                    symbol_at.insert(offset, sym.clone());
                }

                type_defs.insert(td.name.clone(), td.clone());
                ident_spans.push((td.name_span.clone(), IdentContext::TypeRef));

                if let TypeDefKind::Struct(fields) = &td.kind {
                    for field in fields {
                        let field_sym = SymbolInfo {
                            name: field.name.clone(),
                            kind: SymbolKind::Field,
                            span: field.span.clone(),
                            type_str: Some(format!("{}: {}", field.name, field.ty.display())),
                            doc: String::new(),
                        };
                        symbols.push(field_sym.clone());
                        ident_spans.push((field.name_span.clone(), IdentContext::FieldDef));
                        for offset in field.span.range() {
                            symbol_at.insert(offset, field_sym.clone());
                        }
                    }
                }
                if let TypeDefKind::Enum(variants) = &td.kind {
                    for v in variants {
                        let v_sym = SymbolInfo {
                            name: v.name.clone(),
                            kind: SymbolKind::EnumVariant,
                            span: v.span.clone(),
                            type_str: Some(format!("{}.{}", td.name, v.name)),
                            doc: String::new(),
                        };
                        symbols.push(v_sym.clone());
                        ident_spans.push((v.name_span.clone(), IdentContext::EnumVariant));
                        for offset in v.span.range() {
                            symbol_at.insert(offset, v_sym.clone());
                        }
                    }
                }
                if let TypeDefKind::UnionConstruct(variants) = &td.kind {
                    for v in variants {
                        let v_sym = SymbolInfo {
                            name: v.name.clone(),
                            kind: SymbolKind::UnionVariant,
                            span: v.span.clone(),
                            type_str: Some(format!("{}({})", v.name, v.ty.display())),
                            doc: String::new(),
                        };
                        symbols.push(v_sym.clone());
                        ident_spans.push((v.name_span.clone(), IdentContext::UnionVariant));
                        for offset in v.span.range() {
                            symbol_at.insert(offset, v_sym.clone());
                        }
                    }
                }
            }
            Item::Use(local_name, mod_path, span) => {
                let sym = SymbolInfo {
                    name: local_name.clone(),
                    kind: SymbolKind::Module,
                    span: span.clone(),
                    type_str: Some(format!("use(\"{}\")", mod_path)),
                    doc: format!("Module: `{}`", mod_path),
                };
                symbols.push(sym.clone());
                for offset in span.range() {
                    symbol_at.insert(offset, sym.clone());
                }
            }
            Item::Test(tb) => {
                let sym = SymbolInfo {
                    name: "test".to_string(),
                    kind: SymbolKind::Function,
                    span: tb.span.clone(),
                    type_str: Some("test { ... }".to_string()),
                    doc: "Test block".to_string(),
                };
                symbols.push(sym);
            }
        }
    }

    for item in &module.items {
        match item {
            Item::Function(f) => {
                if let Some(body) = &f.body {
                    walk_expr(
                        source,
                        body,
                        &mut ident_spans,
                        &fn_by_name,
                        &type_defs,
                        &mut resolves_to,
                    );
                }
            }
            Item::Test(tb) => {
                walk_expr(
                    source,
                    &tb.body,
                    &mut ident_spans,
                    &fn_by_name,
                    &type_defs,
                    &mut resolves_to,
                );
            }
            _ => {}
        }
    }

    let var_scopes = build_scopes(&module);

    DocumentAnalysis {
        module,
        symbols,
        ident_spans,
        fn_by_name,
        type_defs,
        var_scopes,
        symbol_at,
        resolves_to,
    }
}

fn expr_source(source: &str, expr: &Expr) -> String {
    let span = expr.span();
    let src: Vec<char> = source.chars().collect();
    if span.start < src.len() && span.end <= src.len() && span.end > span.start {
        src[span.start..span.end].iter().collect()
    } else {
        String::new()
    }
}

fn walk_expr(
    source: &str,
    expr: &Expr,
    ident_spans: &mut Vec<(Span, IdentContext)>,
    fn_by_name: &HashMap<String, Function>,
    type_defs: &HashMap<String, TypeDef>,
    resolves_to: &mut HashMap<Span, Span>,
) {
    match expr {
        Expr::Ident(name, span) => {
            let ctx = if fn_by_name.contains_key(name) {
                IdentContext::FunctionCall
            } else if type_defs.contains_key(name) {
                IdentContext::TypeRef
            } else {
                IdentContext::VariableRef
            };
            ident_spans.push((span.clone(), ctx));

            if let Some(f) = fn_by_name.get(name) {
                resolves_to.insert(span.clone(), f.name_span.clone());
            } else if let Some(td) = type_defs.get(name) {
                resolves_to.insert(span.clone(), td.name_span.clone());
            }
        }
        Expr::MethodCall(_, method_name, args, span) => {
            ident_spans.push((span.clone(), IdentContext::MethodCall(method_name.clone())));
            for arg in args {
                walk_expr(source, arg, ident_spans, fn_by_name, type_defs, resolves_to);
            }
        }
        Expr::Call(callee, args, _) => {
            walk_expr(
                source,
                callee,
                ident_spans,
                fn_by_name,
                type_defs,
                resolves_to,
            );
            for arg in args {
                walk_expr(source, arg, ident_spans, fn_by_name, type_defs, resolves_to);
            }
        }
        Expr::FieldAccess(obj, field, span) => {
            walk_expr(source, obj, ident_spans, fn_by_name, type_defs, resolves_to);
            ident_spans.push((span.clone(), IdentContext::FieldAccess));
        }
        Expr::BinaryOp(left, _, right, _) => {
            walk_expr(
                source,
                left,
                ident_spans,
                fn_by_name,
                type_defs,
                resolves_to,
            );
            walk_expr(
                source,
                right,
                ident_spans,
                fn_by_name,
                type_defs,
                resolves_to,
            );
        }
        Expr::UnaryOp(_, operand, _) => {
            walk_expr(
                source,
                operand,
                ident_spans,
                fn_by_name,
                type_defs,
                resolves_to,
            );
        }
        Expr::If(cond, then, else_opt, _) => {
            walk_expr(
                source,
                cond,
                ident_spans,
                fn_by_name,
                type_defs,
                resolves_to,
            );
            walk_expr(
                source,
                then,
                ident_spans,
                fn_by_name,
                type_defs,
                resolves_to,
            );
            if let Some(e) = else_opt {
                walk_expr(source, e, ident_spans, fn_by_name, type_defs, resolves_to);
            }
        }
        Expr::ForLoop(_, header, body, _) => {
            walk_expr(
                source,
                header,
                ident_spans,
                fn_by_name,
                type_defs,
                resolves_to,
            );
            walk_expr(
                source,
                body,
                ident_spans,
                fn_by_name,
                type_defs,
                resolves_to,
            );
        }
        Expr::Match(scrutinee, arms, _) => {
            walk_expr(
                source,
                scrutinee,
                ident_spans,
                fn_by_name,
                type_defs,
                resolves_to,
            );
            for arm in arms {
                walk_pattern(&arm.pattern, ident_spans);
                walk_expr(
                    source,
                    &arm.body,
                    ident_spans,
                    fn_by_name,
                    type_defs,
                    resolves_to,
                );
            }
        }
        Expr::Block(stmts, _) => {
            for stmt in stmts {
                walk_stmt(
                    source,
                    stmt,
                    ident_spans,
                    fn_by_name,
                    type_defs,
                    resolves_to,
                );
            }
        }
        Expr::Trust(inner, _) => {
            walk_expr(
                source,
                inner,
                ident_spans,
                fn_by_name,
                type_defs,
                resolves_to,
            );
        }
        Expr::Return(Some(e), _) => {
            walk_expr(source, e, ident_spans, fn_by_name, type_defs, resolves_to);
        }
        Expr::Cast(e, _, _) => {
            walk_expr(source, e, ident_spans, fn_by_name, type_defs, resolves_to);
        }
        Expr::SomeVariant(e, _) | Expr::OkVariant(e, _) | Expr::ErrVariant(e, _) => {
            walk_expr(source, e, ident_spans, fn_by_name, type_defs, resolves_to);
        }
        Expr::WhenExpr(_, body, _) => {
            walk_expr(
                source,
                body,
                ident_spans,
                fn_by_name,
                type_defs,
                resolves_to,
            );
        }
        Expr::Deref(e, _) | Expr::AddrOf(e, _) | Expr::Propagate(e, _) | Expr::Grouped(e, _) => {
            walk_expr(source, e, ident_spans, fn_by_name, type_defs, resolves_to);
        }
        Expr::StructLit(fields, _) => {
            for (_, e) in fields {
                walk_expr(source, e, ident_spans, fn_by_name, type_defs, resolves_to);
            }
        }
        Expr::ArrayLit(elems, _) => {
            for e in elems {
                walk_expr(source, e, ident_spans, fn_by_name, type_defs, resolves_to);
            }
        }
        Expr::Intrinsic(_, args, _) => {
            for arg in args {
                walk_expr(source, arg, ident_spans, fn_by_name, type_defs, resolves_to);
            }
        }
        Expr::Index(arr, idx, _) => {
            walk_expr(source, arr, ident_spans, fn_by_name, type_defs, resolves_to);
            walk_expr(source, idx, ident_spans, fn_by_name, type_defs, resolves_to);
        }
        _ => {}
    }
}

fn walk_pattern(pattern: &Pattern, ident_spans: &mut Vec<(Span, IdentContext)>) {
    match pattern {
        Pattern::Ident(_, span) => {
            ident_spans.push((span.clone(), IdentContext::VariableDef));
        }
        Pattern::SomePat(inner, _) | Pattern::OkPat(inner, _) | Pattern::ErrPat(inner, _) => {
            walk_pattern(inner, ident_spans);
        }
        Pattern::ConstructPat(_, args, _) => {
            for arg in args {
                walk_pattern(arg, ident_spans);
            }
        }
        Pattern::TuplePat(args, _) => {
            for arg in args {
                walk_pattern(arg, ident_spans);
            }
        }
        Pattern::TypePat(_, span) => {
            ident_spans.push((span.clone(), IdentContext::TypeRef));
        }
        Pattern::StringPat(_, _) | Pattern::Discard(_) | Pattern::ElsePat(_) => {}
    }
}

fn walk_stmt(
    source: &str,
    stmt: &Stmt,
    ident_spans: &mut Vec<(Span, IdentContext)>,
    fn_by_name: &HashMap<String, Function>,
    type_defs: &HashMap<String, TypeDef>,
    resolves_to: &mut HashMap<Span, Span>,
) {
    match stmt {
        Stmt::Let(_, ty, expr, span, _) => {
            ident_spans.push((span.clone(), IdentContext::VariableDef));
            if let Some(t) = ty {
                ident_spans.push((t.span(), IdentContext::TypeRef));
            }
            walk_expr(
                source,
                expr,
                ident_spans,
                fn_by_name,
                type_defs,
                resolves_to,
            );
        }
        Stmt::Expr(e, _) => {
            walk_expr(source, e, ident_spans, fn_by_name, type_defs, resolves_to);
        }
        Stmt::Assign(lhs, rhs, _) => {
            walk_expr(source, lhs, ident_spans, fn_by_name, type_defs, resolves_to);
            walk_expr(source, rhs, ident_spans, fn_by_name, type_defs, resolves_to);
        }
        Stmt::Use(_, _, _) => {}
    }
}

fn build_scopes(module: &Module) -> Vec<Scope> {
    let mut scopes = Vec::new();

    for item in &module.items {
        match item {
            Item::Function(f) => {
                let mut variables = HashMap::new();
                for p in &f.params {
                    variables.insert(p.name.clone(), (p.ty.display(), p.name_span.clone()));
                }
                if let Some(body) = &f.body {
                    collect_vars_from_expr(body, &mut variables);
                }
                scopes.push(Scope {
                    start: f.span.start,
                    end: f.span.end,
                    variables,
                });
            }
            Item::TypeDef(td) => {
                scopes.push(Scope {
                    start: td.span.start,
                    end: td.span.end,
                    variables: HashMap::new(),
                });
            }
            Item::Test(tb) => {
                let mut variables = HashMap::new();
                collect_vars_from_expr(&tb.body, &mut variables);
                scopes.push(Scope {
                    start: tb.span.start,
                    end: tb.span.end,
                    variables,
                });
            }
            _ => {}
        }
    }

    scopes
}

fn collect_vars_from_expr(expr: &Expr, vars: &mut HashMap<String, (String, Span)>) {
    if let Expr::Block(stmts, span) = expr {
        for stmt in stmts {
            if let Stmt::Let(name, ty, _, let_span, _) = stmt {
                let type_str = ty
                    .as_ref()
                    .map(|t| t.display())
                    .unwrap_or_else(|| "inferred".to_string());
                vars.insert(name.clone(), (type_str, let_span.clone()));
            }
            match stmt {
                Stmt::Expr(e, _) => collect_vars_from_expr(e, vars),
                Stmt::Assign(_, rhs, _) => collect_vars_from_expr(rhs, vars),
                _ => {}
            }
        }
    }
}

pub fn hover_at(analysis: &DocumentAnalysis, offset: usize, source: &str) -> Option<HoverResult> {
    if let Some(sym) = analysis.symbol_at.get(&offset) {
        return Some(HoverResult {
            signature: sym.type_str.clone().unwrap_or(sym.name.clone()),
            documentation: sym.doc.clone(),
        });
    }

    for (span, ctx) in &analysis.ident_spans {
        if offset >= span.start && offset < span.end {
            match ctx {
                IdentContext::FunctionCall | IdentContext::FunctionDef => {
                    if let Some(f) = analysis
                        .fn_by_name
                        .values()
                        .find(|f| offset >= f.name_span.start && offset < f.name_span.end)
                    {
                        let ret_display = if f.returns_untrusted {
                            format!("{}!", f.return_type.display())
                        } else {
                            f.return_type.display()
                        };

                        let mut sig = format!(
                            "fn {}({}) -> {}",
                            f.name,
                            f.params
                                .iter()
                                .map(|p| format!("{}: {}", p.name, p.ty.display()))
                                .collect::<Vec<_>>()
                                .join(", "),
                            ret_display
                        );

                        if f.returns_untrusted {
                            sig.push_str("\n\n(!) **UNTRUSTED FUNCTION** — may have unintended side-effects and bypasses the trust system.");
                        }

                        let mut doc = f.doc_comments.join("\n");

                        if let Some(pre) = &f.pre {
                            if !pre.conditions.is_empty() {
                                if !doc.is_empty() {
                                    doc.push_str("\n\n");
                                }
                                let guard_label = if pre.guarded { " (guarded)" } else { "" };
                                doc.push_str(&format!("**Preconditions{}:**\n", guard_label));
                                for cond in &pre.conditions {
                                    let expr_text = expr_source(source, &cond.expr);
                                    doc.push_str(&format!("- `{}`: `{}`\n", cond.name, expr_text));
                                }
                            }
                        }

                        if let Some(post) = &f.post {
                            if !post.conditions.is_empty() {
                                if !doc.is_empty() {
                                    doc.push_str("\n\n");
                                }
                                doc.push_str("**Postconditions:**\n");
                                for cond in &post.conditions {
                                    let expr_text = expr_source(source, &cond.expr);
                                    doc.push_str(&format!("- `{}`: `{}`\n", cond.name, expr_text));
                                }
                            }
                        }

                        return Some(HoverResult {
                            signature: sig,
                            documentation: doc,
                        });
                    }
                }
                IdentContext::TypeRef => {
                    if let Some(td) = analysis
                        .type_defs
                        .values()
                        .find(|td| offset >= td.name_span.start && offset < td.name_span.end)
                    {
                        let kind_str = match &td.kind {
                            TypeDefKind::Struct(fields) => {
                                let fields_str = fields
                                    .iter()
                                    .map(|f| {
                                        let mut_s = if f.is_mut { "mut " } else { "" };
                                        format!("    {}{}: {}", mut_s, f.name, f.ty.display())
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                format!("type definition {} {{\n{}\n}}", td.name, fields_str)
                            }
                            TypeDefKind::Union(types) => {
                                format!(
                                    "union {} = {}",
                                    td.name,
                                    types
                                        .iter()
                                        .map(|t| t.display())
                                        .collect::<Vec<_>>()
                                        .join(" | ")
                                )
                            }
                            TypeDefKind::UnionConstruct(variants) => {
                                format!(
                                    "union {} = {{\n{}\n}}",
                                    td.name,
                                    variants
                                        .iter()
                                        .map(|v| format!("    {}({})", v.name, v.ty.display()))
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                )
                            }
                            TypeDefKind::Enum(variants) => {
                                format!(
                                    "enum {} = {{\n{}\n}}",
                                    td.name,
                                    variants
                                        .iter()
                                        .map(|v| format!("    {}", v.name))
                                        .collect::<Vec<_>>()
                                        .join(",\n")
                                )
                            }
                        };
                        let doc = td.doc_comments.join("\n");
                        return Some(HoverResult {
                            signature: kind_str,
                            documentation: doc,
                        });
                    }
                }
                IdentContext::Parameter => {
                    for f in analysis.fn_by_name.values() {
                        for p in &f.params {
                            if offset >= p.name_span.start && offset < p.name_span.end {
                                return Some(HoverResult {
                                    signature: format!("{}: {}", p.name, p.ty.display()),
                                    documentation: format!("Parameter of `{}`", f.name),
                                });
                            }
                        }
                    }
                }
                IdentContext::VariableRef => {
                    for scope in &analysis.var_scopes {
                        if offset >= scope.start && offset < scope.end {
                            for (name, (type_str, def_span)) in &scope.variables {
                                let src_chars: Vec<char> = source.chars().collect();
                                if offset < src_chars.len() {
                                    let _ = def_span; // suppress warning
                                }
                            }
                        }
                    }
                }
                IdentContext::FieldAccess => {}
                IdentContext::MethodCall(method_name) => {
                    for f in analysis.fn_by_name.values() {
                        if f.name == *method_name {
                            let ret_display = if f.returns_untrusted {
                                format!("{}!", f.return_type.display())
                            } else {
                                f.return_type.display()
                            };
                            let sig = if let Some(self_ty) = &f.self_type {
                                format!(
                                    "fn ({}).{}({}) -> {}",
                                    self_ty,
                                    f.name,
                                    f.params
                                        .iter()
                                        .map(|p| format!("{}: {}", p.name, p.ty.display()))
                                        .collect::<Vec<_>>()
                                        .join(", "),
                                    ret_display
                                )
                            } else {
                                format!(
                                    "fn {}({}) -> {}",
                                    f.name,
                                    f.params
                                        .iter()
                                        .map(|p| format!("{}: {}", p.name, p.ty.display()))
                                        .collect::<Vec<_>>()
                                        .join(", "),
                                    ret_display
                                )
                            };

                            let mut doc = f.doc_comments.join("\n");
                            if f.returns_untrusted {
                                if !doc.is_empty() {
                                    doc.push_str("\n\n");
                                }
                                doc.push_str(
                                    "⚠️ **Untrusted** — may have unintended side-effects.",
                                );
                            }
                            if let Some(pre) = &f.pre {
                                if !pre.conditions.is_empty() {
                                    if !doc.is_empty() {
                                        doc.push_str("\n\n");
                                    }
                                    doc.push_str("**Preconditions:**\n");
                                    for cond in &pre.conditions {
                                        doc.push_str(&format!(
                                            "- `{}`: `{}`\n",
                                            cond.name,
                                            expr_source(source, &cond.expr)
                                        ));
                                    }
                                }
                            }

                            return Some(HoverResult {
                                signature: sig,
                                documentation: doc,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let src_chars: Vec<char> = source.chars().collect();
    if offset < src_chars.len() {
        let word = extract_word_at(&src_chars, offset);
        if word == "trust" {
            return Some(HoverResult {
                signature: "trust".to_string(),
                documentation: "**trust** — Asserts that calling an untrusted (`!`) function is intentional.\n\n\
                    Allows calling a `!`-suffixed (untrusted) function from a pure (non-`!`) context.\n\
                    This tells the compiler: \"I know what I'm doing.\"\n\n\
                    Untrusted functions may have unintended side-effects and bypass the trust system."
                    .to_string(),
            });
        }
        if word == "pure" || word == "fn" {
            return Some(HoverResult {
                signature: word.clone(),
                documentation: if word == "fn" {
                    "**fn** — Function definition keyword.\n\n\
                    Functions without a `!` suffix on the return type are **pure** (trusted).\n\
                    Pure functions cannot directly call untrusted functions unless wrapped in `trust`."
                        .to_string()
                } else {
                    "**pure** — A function without `!` on its return type is considered pure/trusted."
                        .to_string()
                },
            });
        }
        if word == "pre" {
            return Some(HoverResult {
                signature: "pre".to_string(),
                documentation: "**pre** — Preconditions block for a function.\n\n\
                    Lists named boolean expressions that must be true when the function is called.\n\
                    Use `guarded pre` for preconditions that the function itself verifies."
                    .to_string(),
            });
        }
        if word == "post" {
            return Some(HoverResult {
                signature: "post".to_string(),
                documentation: "**post** — Postconditions block for a function.\n\n\
                    Lists named boolean expressions that must be true when the function returns."
                    .to_string(),
            });
        }
        if word == "guarded" {
            return Some(HoverResult {
                signature: "guarded".to_string(),
                documentation: "**guarded** — Modifies a `pre` block to indicate the function itself \
                    verifies the precondition at runtime, rather than requiring the caller to guarantee it."
                    .to_string(),
            });
        }
        if word == "mut" {
            return Some(HoverResult {
                signature: "mut".to_string(),
                documentation: "**mut** — Marks a variable, field, or type as mutable.\n\n\
                    `val x: mut i32 = 10` — mutable variable\n\
                    `y: mut i32` — mutable struct field\n\
                    A `val` binding is always immutable regardless of `mut` on the type."
                    .to_string(),
            });
        }
        if word == "ref" {
            return Some(HoverResult {
                signature: "ref".to_string(),
                documentation: "**ref** — Raw reference (pointer) type.\n\n\
                    `ref T` is a raw reference to type T. Used for FFI and low-level memory access.\n\
                    `ref void` is an untyped raw pointer."
                    .to_string(),
            });
        }
        if word == "type" {
            return Some(HoverResult {
                signature: "type".to_string(),
                documentation: "**type** — Type definition keyword.\n\n\
                    `type Foo = { x: i32, y: mut i32 }` defines a struct type."
                    .to_string(),
            });
        }
        if word == "union" {
            return Some(HoverResult {
                signature: "union".to_string(),
                documentation: "**union** — Tagged union type definition.\n\n\
                    `union U = { Int32(i32) | Str(String) }` defines a sum type with named variants."
                    .to_string(),
            });
        }
        if word == "enum" {
            return Some(HoverResult {
                signature: "enum".to_string(),
                documentation: "**enum** — Enumeration type definition.\n\n\
                    `enum E = { A, B, C }` defines a C-like enum."
                    .to_string(),
            });
        }
        if word == "val" {
            return Some(HoverResult {
                signature: "val".to_string(),
                documentation: "**val** — Immutable value binding.\n\n\
                    `val x = 10` or `val x: i32 = 10`"
                    .to_string(),
            });
        }
        if word == "match" {
            return Some(HoverResult {
                signature: "match".to_string(),
                documentation: "**match** — Pattern matching expression.\n\n\
                    Supports `some x`, `ok v`, `err e`, type patterns, string patterns, \
                    constructor patterns with destructuring, and `else` as catch-all."
                    .to_string(),
            });
        }
        if word == "some" {
            return Some(HoverResult {
                signature: "some".to_string(),
                documentation: "**some** — The `some` variant of the `option[T]` type.\n\n\
                    Used to wrap a value: `some 10`"
                    .to_string(),
            });
        }
        if word == "none" {
            return Some(HoverResult {
                signature: "none".to_string(),
                documentation: "**none** — The `none` variant of the `option[T]` type.\n\n\
                    Represents absence of a value."
                    .to_string(),
            });
        }
        if word == "ok" {
            return Some(HoverResult {
                signature: "ok".to_string(),
                documentation: "**ok** — The `ok` variant of the `result[T, E]` type.\n\n\
                    Used to wrap a success value: `ok 42`"
                    .to_string(),
            });
        }
        if word == "err" {
            return Some(HoverResult {
                signature: "err".to_string(),
                documentation: "**err** — The `err` variant of the `result[T, E]` type.\n\n\
                    Used to wrap an error value: `err ParseError.Empty`"
                    .to_string(),
            });
        }
        if word == "option" {
            return Some(HoverResult {
                signature: "option[T]".to_string(),
                documentation: "**option[T]** — Optional type.\n\n\
                    Either `some value` or `none`."
                    .to_string(),
            });
        }
        if word == "result" {
            return Some(HoverResult {
                signature: "result[T, E]".to_string(),
                documentation: "**result[T, E]** — Result type for fallible operations.\n\n\
                    Either `ok value` or `err error`.\n\
                    Supports `?` propagation operator."
                    .to_string(),
            });
        }
        if word == "when" {
            return Some(HoverResult {
                signature: "when".to_string(),
                documentation: "**when** — Conditional compilation.\n\n\
                    `when linux { ... }` — only compiled on Linux.\n\
                    Supported platforms: windows, linux, darwin, dragonflybsd, freebsd, openbsd, netbsd."
                    .to_string(),
            });
        }
        if word == "extern" {
            return Some(HoverResult {
                signature: "extern".to_string(),
                documentation: "**extern** — External FFI declaration.\n\n\
                    `extern (C) fn malloc(size: usize) ref void! = \"malloc\"`\n\
                    All extern functions must have `!` on their return type (they are inherently untrusted)."
                    .to_string(),
            });
        }
        if word == "pub" {
            return Some(HoverResult {
                signature: "pub".to_string(),
                documentation: "**pub** — Public visibility modifier.\n\n\
                    Makes a function or type accessible from other modules."
                    .to_string(),
            });
        }
        if word == "return" {
            return Some(HoverResult {
                signature: "return".to_string(),
                documentation: "**return** — Return from the current function.\n\n\
                    `return expr` or just `return` for void functions."
                    .to_string(),
            });
        }
        if word == "if" {
            return Some(HoverResult {
                signature: "if".to_string(),
                documentation: "**if** — Conditional expression.\n\n\
                    Supports `elif` and `else` branches."
                    .to_string(),
            });
        }
        if word == "for" {
            return Some(HoverResult {
                signature: "for".to_string(),
                documentation: "**for** — Loop construct.\n\n\
                    - C-style: `for i = 0; i < 10; i++ { ... }`\n\
                    - For-in: `for x in xs { ... }`\n\
                    - Infinite: `for { ... }`"
                    .to_string(),
            });
        }
        if word == "break" {
            return Some(HoverResult {
                signature: "break".to_string(),
                documentation: "**break** — Exit the current loop.".to_string(),
            });
        }
        if word == "continue" {
            return Some(HoverResult {
                signature: "continue".to_string(),
                documentation: "**continue** — Skip to the next iteration of the current loop."
                    .to_string(),
            });
        }
        if word == "use" {
            return Some(HoverResult {
                signature: "use".to_string(),
                documentation: "**use** — Import a module.\n\n\
                    `val std = use(\"std\")` imports the standard library.\n\
                    `val mod = use(\"./submod/sub.sx\")` imports a relative file."
                    .to_string(),
            });
        }
        if word == "test" {
            return Some(HoverResult {
                signature: "test".to_string(),
                documentation: "**test** — Test block.\n\n\
                    Contains assertions that are run during testing."
                    .to_string(),
            });
        }
        if word == "assert" {
            return Some(HoverResult {
                signature: "assert".to_string(),
                documentation: "**assert** — Runtime assertion.\n\n\
                    `assert expr` — panics if expr is false."
                    .to_string(),
            });
        }
        if word == "defer" {
            return Some(HoverResult {
                signature: "defer".to_string(),
                documentation: "**defer** — Deferred execution.\n\n\
                    `defer { ... }` — code runs when the current scope exits."
                    .to_string(),
            });
        }
        if word == "void" {
            return Some(HoverResult {
                signature: "void".to_string(),
                documentation: "**void** — Unit type. Represents no value / no return.".to_string(),
            });
        }
        if word == "bool" {
            return Some(HoverResult {
                signature: "bool".to_string(),
                documentation: "**bool** — Boolean type. Values: `true` or `false`.".to_string(),
            });
        }
        if word == "deref" {
            return Some(HoverResult {
                signature: "deref".to_string(),
                documentation: "**deref** — Dereference a raw pointer.\n\n\
                    `deref(ptr) = 42` writes 42 to the memory pointed to by `ptr`."
                    .to_string(),
            });
        }
        if word == "addr" {
            return Some(HoverResult {
                signature: "addr".to_string(),
                documentation: "**addr** — Take the address of a value.\n\n\
                    `addr(x)` returns a raw reference to `x`."
                    .to_string(),
            });
        }
    }

    None
}

fn extract_word_at(chars: &[char], offset: usize) -> String {
    if offset >= chars.len() {
        return String::new();
    }

    let mut start = offset;
    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        start -= 1;
    }
    let mut end = offset;
    while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
        end += 1;
    }

    if end > start {
        chars[start..end].iter().collect()
    } else {
        String::new()
    }
}

#[derive(Debug, Clone)]
pub struct HoverResult {
    pub signature: String,
    pub documentation: String,
}

pub fn signature_help_at(
    analysis: &DocumentAnalysis,
    offset: usize,
) -> Option<SignatureHelpResult> {
    let mut best: Option<(Function, usize)> = None;

    for f in analysis.fn_by_name.values() {
        if let Some(body) = &f.body {
            if let Some((fn_span, arg_idx)) = find_call_at(body, offset) {
                best = Some((f.clone(), arg_idx));
            }
        }
    }

    for f in analysis.fn_by_name.values() {
        if let Some((_, arg_idx)) = find_call_in_fn(f, offset) {
            best = Some((f.clone(), arg_idx));
        }
    }

    if let Some((func, active_param)) = best {
        let param_count = func.params.len();
        let label = format!(
            "{}({})",
            func.name,
            func.params
                .iter()
                .map(|p| format!("{}: {}", p.name, p.ty.display()))
                .collect::<Vec<_>>()
                .join(", ")
        );

        let parameters: Vec<SignatureParam> = func
            .params
            .iter()
            .map(|p| SignatureParam {
                label: format!("{}: {}", p.name, p.ty.display()),
                documentation: String::new(),
            })
            .collect();

        let mut doc = func.doc_comments.join("\n");
        if func.returns_untrusted {
            if !doc.is_empty() {
                doc.push_str("\n\n");
            }
            doc.push_str("⚠️ **Untrusted function**");
        }

        let active = if active_param < param_count {
            active_param
        } else {
            param_count.saturating_sub(1)
        };

        Some(SignatureHelpResult {
            label,
            parameters,
            active_parameter: active,
            documentation: doc,
        })
    } else {
        None
    }
}

fn find_call_in_fn(f: &Function, offset: usize) -> Option<(Span, usize)> {
    fn search_expr(expr: &Expr, offset: usize) -> Option<(Span, usize)> {
        match expr {
            Expr::Call(callee, args, span) => {
                if offset >= span.start && offset <= span.end {
                    let arg_idx = count_args_at(expr, offset);
                    return Some((span.clone(), arg_idx));
                }
                for arg in args {
                    if let Some(r) = search_expr(arg, offset) {
                        return Some(r);
                    }
                }
                if let Expr::Ident(_, cs) = callee.as_ref() {
                    if offset >= cs.start && offset < cs.end {
                        return Some((span.clone(), 0));
                    }
                }
            }
            Expr::MethodCall(_, _, args, span) => {
                if offset >= span.start && offset <= span.end {
                    let arg_idx = count_args_at(expr, offset);
                    return Some((span.clone(), arg_idx));
                }
                for arg in args {
                    if let Some(r) = search_expr(arg, offset) {
                        return Some(r);
                    }
                }
            }
            Expr::Block(stmts, _) => {
                for stmt in stmts {
                    if let Some(r) = search_stmt(stmt, offset) {
                        return Some(r);
                    }
                }
            }
            Expr::BinaryOp(left, _, right, _) => {
                if let Some(r) = search_expr(left, offset) {
                    return Some(r);
                }
                if let Some(r) = search_expr(right, offset) {
                    return Some(r);
                }
            }
            Expr::If(cond, then, else_opt, _) => {
                if let Some(r) = search_expr(cond, offset) {
                    return Some(r);
                }
                if let Some(r) = search_expr(then, offset) {
                    return Some(r);
                }
                if let Some(e) = else_opt {
                    if let Some(r) = search_expr(e, offset) {
                        return Some(r);
                    }
                }
            }
            Expr::ForLoop(_, header, body, _) => {
                if let Some(r) = search_expr(header, offset) {
                    return Some(r);
                }
                if let Some(r) = search_expr(body, offset) {
                    return Some(r);
                }
            }
            Expr::Match(scrutinee, arms, _) => {
                if let Some(r) = search_expr(scrutinee, offset) {
                    return Some(r);
                }
                for arm in arms {
                    if let Some(r) = search_expr(&arm.body, offset) {
                        return Some(r);
                    }
                }
            }
            Expr::Trust(inner, _) => {
                if let Some(r) = search_expr(inner, offset) {
                    return Some(r);
                }
            }
            Expr::Return(Some(e), _) => {
                if let Some(r) = search_expr(e, offset) {
                    return Some(r);
                }
            }
            Expr::SomeVariant(e, _) | Expr::OkVariant(e, _) | Expr::ErrVariant(e, _) => {
                if let Some(r) = search_expr(e, offset) {
                    return Some(r);
                }
            }
            _ => {}
        }
        None
    }

    f.body.as_ref().and_then(|b| search_expr(b, offset))
}

fn search_stmt(stmt: &Stmt, offset: usize) -> Option<(Span, usize)> {
    match stmt {
        Stmt::Expr(e, _) => search_expr(e, offset),
        Stmt::Assign(_, rhs, _) => search_expr(rhs, offset),
        Stmt::Let(_, _, e, _, _) => search_expr(e, offset),
        _ => None,
    }
}

fn search_expr(expr: &Expr, offset: usize) -> Option<(Span, usize)> {
    match expr {
        Expr::Call(callee, args, span) => {
            if offset >= span.start && offset <= span.end {
                let arg_idx = count_args_at(expr, offset);
                return Some((span.clone(), arg_idx));
            }
            for arg in args {
                if let Some(r) = search_expr(arg, offset) {
                    return Some(r);
                }
            }
        }
        Expr::MethodCall(_, _, args, span) => {
            if offset >= span.start && offset <= span.end {
                let arg_idx = count_args_at(expr, offset);
                return Some((span.clone(), arg_idx));
            }
            for arg in args {
                if let Some(r) = search_expr(arg, offset) {
                    return Some(r);
                }
            }
        }
        Expr::Block(stmts, _) => {
            for stmt in stmts {
                if let Some(r) = search_stmt(stmt, offset) {
                    return Some(r);
                }
            }
        }
        Expr::BinaryOp(left, _, right, _) => {
            if let Some(r) = search_expr(left, offset) {
                return Some(r);
            }
            if let Some(r) = search_expr(right, offset) {
                return Some(r);
            }
        }
        Expr::If(cond, then, else_opt, _) => {
            if let Some(r) = search_expr(cond, offset) {
                return Some(r);
            }
            if let Some(r) = search_expr(then, offset) {
                return Some(r);
            }
            if let Some(e) = else_opt {
                if let Some(r) = search_expr(e, offset) {
                    return Some(r);
                }
            }
        }
        Expr::ForLoop(_, header, body, _) => {
            if let Some(r) = search_expr(header, offset) {
                return Some(r);
            }
            if let Some(r) = search_expr(body, offset) {
                return Some(r);
            }
        }
        Expr::Match(scrutinee, arms, _) => {
            if let Some(r) = search_expr(scrutinee, offset) {
                return Some(r);
            }
            for arm in arms {
                if let Some(r) = search_expr(&arm.body, offset) {
                    return Some(r);
                }
            }
        }
        Expr::Trust(inner, _) => {
            if let Some(r) = search_expr(inner, offset) {
                return Some(r);
            }
        }
        Expr::Return(Some(e), _) => {
            if let Some(r) = search_expr(e, offset) {
                return Some(r);
            }
        }
        Expr::SomeVariant(e, _) | Expr::OkVariant(e, _) | Expr::ErrVariant(e, _) => {
            if let Some(r) = search_expr(e, offset) {
                return Some(r);
            }
        }
        _ => {}
    }
    None
}

fn count_args_at(expr: &Expr, offset: usize) -> usize {
    match expr {
        Expr::Call(_, _, span) | Expr::MethodCall(_, _, _, span) => {
            let source: Vec<char> = "".chars().collect();
            let _ = (source, span);
            0
        }
        _ => 0,
    }
}

fn find_call_at(_expr: &Expr, _offset: usize) -> Option<(Span, usize)> {
    None
}

#[derive(Debug, Clone)]
pub struct SignatureHelpResult {
    pub label: String,
    pub parameters: Vec<SignatureParam>,
    pub active_parameter: usize,
    pub documentation: String,
}

#[derive(Debug, Clone)]
pub struct SignatureParam {
    pub label: String,
    pub documentation: String,
}

pub fn document_symbols(analysis: &DocumentAnalysis) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();

    for item in &analysis.module.items {
        match item {
            Item::Function(f) => {
                let mut children = Vec::new();
                for p in &f.params {
                    children.push(DocumentSymbol {
                        name: p.name.clone(),
                        kind: lsp_types::SymbolKind::VARIABLE,
                        span: p.span.clone(),
                        children: Vec::new(),
                        detail: Some(p.ty.display()),
                    });
                }
                if let Some(pre) = &f.pre {
                    for cond in &pre.conditions {
                        children.push(DocumentSymbol {
                            name: format!("pre: {}", cond.name),
                            kind: lsp_types::SymbolKind::EVENT,
                            span: cond.name_span.clone(),
                            children: Vec::new(),
                            detail: Some(format!(
                                "precondition{}",
                                if pre.guarded { " (guarded)" } else { "" }
                            )),
                        });
                    }
                }
                if let Some(post) = &f.post {
                    for cond in &post.conditions {
                        children.push(DocumentSymbol {
                            name: format!("post: {}", cond.name),
                            kind: lsp_types::SymbolKind::EVENT,
                            span: cond.name_span.clone(),
                            children: Vec::new(),
                            detail: Some("postcondition".to_string()),
                        });
                    }
                }

                let ret_str = if f.returns_untrusted {
                    format!("-> {}!", f.return_type.display())
                } else {
                    format!("-> {}", f.return_type.display())
                };

                symbols.push(DocumentSymbol {
                    name: if f.is_pub {
                        format!("pub fn {}", f.name)
                    } else {
                        format!("fn {}", f.name)
                    },
                    kind: lsp_types::SymbolKind::FUNCTION,
                    span: f.span.clone(),
                    children,
                    detail: Some(ret_str),
                });
            }
            Item::TypeDef(td) => {
                let mut children = Vec::new();
                match &td.kind {
                    TypeDefKind::Struct(fields) => {
                        for field in fields {
                            let detail = if field.is_mut {
                                format!("mut {}", field.ty.display())
                            } else {
                                field.ty.display()
                            };
                            children.push(DocumentSymbol {
                                name: field.name.clone(),
                                kind: lsp_types::SymbolKind::FIELD,
                                span: field.span.clone(),
                                children: Vec::new(),
                                detail: Some(detail),
                            });
                        }
                    }
                    TypeDefKind::Union(types) => {
                        for (i, ty) in types.iter().enumerate() {
                            children.push(DocumentSymbol {
                                name: format!("variant {}", i),
                                kind: lsp_types::SymbolKind::ENUM_MEMBER,
                                span: ty.span(),
                                children: Vec::new(),
                                detail: Some(ty.display()),
                            });
                        }
                    }
                    TypeDefKind::UnionConstruct(variants) => {
                        for v in variants {
                            children.push(DocumentSymbol {
                                name: v.name.clone(),
                                kind: lsp_types::SymbolKind::ENUM_MEMBER,
                                span: v.span.clone(),
                                children: Vec::new(),
                                detail: Some(format!("({})", v.ty.display())),
                            });
                        }
                    }
                    TypeDefKind::Enum(variants) => {
                        for v in variants {
                            children.push(DocumentSymbol {
                                name: v.name.clone(),
                                kind: lsp_types::SymbolKind::ENUM_MEMBER,
                                span: v.span.clone(),
                                children: Vec::new(),
                                detail: None,
                            });
                        }
                    }
                }

                let kind_label = match &td.kind {
                    TypeDefKind::Struct(_) => "struct",
                    TypeDefKind::Union(_) => "union",
                    TypeDefKind::UnionConstruct(_) => "union",
                    TypeDefKind::Enum(_) => "enum",
                };

                symbols.push(DocumentSymbol {
                    name: if td.is_pub {
                        format!("pub {} {}", kind_label, td.name)
                    } else {
                        format!("{} {}", kind_label, td.name)
                    },
                    kind: lsp_types::SymbolKind::CLASS,
                    span: td.span.clone(),
                    children,
                    detail: Some(format!("{} {}", kind_label, td.name)),
                });
            }
            Item::Use(name, path, span) => {
                symbols.push(DocumentSymbol {
                    name: format!("use {} = use(\"{}\")", name, path),
                    kind: lsp_types::SymbolKind::MODULE,
                    span: span.clone(),
                    children: Vec::new(),
                    detail: Some(format!("import: {}", path)),
                });
            }
            Item::Test(tb) => {
                symbols.push(DocumentSymbol {
                    name: "test".to_string(),
                    kind: lsp_types::SymbolKind::FUNCTION,
                    span: tb.span.clone(),
                    children: Vec::new(),
                    detail: Some("test block".to_string()),
                });
            }
        }
    }

    symbols
}

#[derive(Debug, Clone)]
pub struct DocumentSymbol {
    pub name: String,
    pub kind: lsp_types::SymbolKind,
    pub span: Span,
    pub children: Vec<DocumentSymbol>,
    pub detail: Option<String>,
}

pub fn goto_definition(analysis: &DocumentAnalysis, offset: usize) -> Option<Span> {
    for (span, def_span) in &analysis.resolves_to {
        if offset >= span.start && offset < span.end {
            return Some(def_span.clone());
        }
    }

    for (span, ctx) in &analysis.ident_spans {
        if offset >= span.start && offset < span.end {
            match ctx {
                IdentContext::FunctionCall | IdentContext::FunctionDef => {
                    let src: Vec<char> = analysis.module.source.chars().collect();
                    if span.start < src.len() {
                        let name: String =
                            src[span.start..span.end.min(src.len())].iter().collect();
                        if let Some(f) = analysis.fn_by_name.get(&name) {
                            return Some(f.name_span.clone());
                        }
                    }
                }
                IdentContext::TypeRef => {
                    let src: Vec<char> = analysis.module.source.chars().collect();
                    if span.start < src.len() {
                        let name: String =
                            src[span.start..span.end.min(src.len())].iter().collect();
                        if let Some(td) = analysis.type_defs.get(&name) {
                            return Some(td.name_span.clone());
                        }
                    }
                }
                IdentContext::VariableRef => {
                    for scope in &analysis.var_scopes {
                        if offset >= scope.start && offset < scope.end {
                            for (_, (_, def_span)) in &scope.variables {
                                if offset >= def_span.start && offset < def_span.end {
                                    return Some(def_span.clone());
                                }
                            }
                        }
                    }
                }
                IdentContext::MethodCall(method_name) => {
                    if let Some(f) = analysis.fn_by_name.get(method_name) {
                        return Some(f.name_span.clone());
                    }
                }
                IdentContext::FieldAccess => {}
                _ => {}
            }
        }
    }

    None
}

pub fn type_at(analysis: &DocumentAnalysis, offset: usize) -> Option<String> {
    if let Some(sym) = analysis.symbol_at.get(&offset) {
        return sym.type_str.clone();
    }

    for (span, ctx) in &analysis.ident_spans {
        if offset >= span.start && offset < span.end {
            match ctx {
                IdentContext::VariableRef => {
                    let src: Vec<char> = analysis.module.source.chars().collect();
                    if span.start < src.len() {
                        let name: String =
                            src[span.start..span.end.min(src.len())].iter().collect();
                        for scope in &analysis.var_scopes {
                            if let Some((type_str, _)) = scope.variables.get(&name) {
                                return Some(type_str.clone());
                            }
                        }
                    }
                }
                IdentContext::Parameter => {
                    for f in analysis.fn_by_name.values() {
                        for p in &f.params {
                            if offset >= p.name_span.start && offset < p.name_span.end {
                                return Some(p.ty.display());
                            }
                        }
                    }
                }
                IdentContext::TypeRef => {
                    let src: Vec<char> = analysis.module.source.chars().collect();
                    if span.start < src.len() {
                        let name: String =
                            src[span.start..span.end.min(src.len())].iter().collect();
                        if let Some(td) = analysis.type_defs.get(&name) {
                            return match &td.kind {
                                TypeDefKind::Struct(fields) => Some(format!(
                                    "type definition {{ {} }}",
                                    fields
                                        .iter()
                                        .map(|f| format!("{}: {}", f.name, f.ty.display()))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )),
                                TypeDefKind::Union(types) => Some(format!(
                                    "union {{ {} }}",
                                    types
                                        .iter()
                                        .map(|t| t.display())
                                        .collect::<Vec<_>>()
                                        .join(" | ")
                                )),
                                TypeDefKind::UnionConstruct(variants) => Some(format!(
                                    "union {{ {} }}",
                                    variants
                                        .iter()
                                        .map(|v| format!("{}({})", v.name, v.ty.display()))
                                        .collect::<Vec<_>>()
                                        .join(" | ")
                                )),
                                TypeDefKind::Enum(variants) => Some(format!(
                                    "enum {{ {} }}",
                                    variants
                                        .iter()
                                        .map(|v| v.name.clone())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )),
                            };
                        }
                    }
                }
                IdentContext::FunctionCall => {
                    let src: Vec<char> = analysis.module.source.chars().collect();
                    if span.start < src.len() {
                        let name: String =
                            src[span.start..span.end.min(src.len())].iter().collect();
                        if let Some(f) = analysis.fn_by_name.get(&name) {
                            let ret = if f.returns_untrusted {
                                format!("{}!", f.return_type.display())
                            } else {
                                f.return_type.display()
                            };
                            return Some(ret);
                        }
                    }
                }
                IdentContext::FieldAccess => {
                    return Some("field access (type inference not yet available)".to_string());
                }
                _ => {}
            }
        }
    }

    let src_chars: Vec<char> = analysis.module.source.chars().collect();
    if offset < src_chars.len() {
        let mut start = offset;
        while start > 0 && src_chars[start - 1].is_ascii_digit() {
            start -= 1;
        }
        let mut end = offset;
        while end < src_chars.len() && (src_chars[end].is_ascii_digit() || src_chars[end] == '.') {
            end += 1;
        }
        if end > start {
            let lit: String = src_chars[start..end].iter().collect();
            if lit.contains('.') {
                return Some("f64".to_string());
            } else {
                return Some("i32 (inferred)".to_string());
            }
        }
    }

    None
}

pub fn completions() -> Vec<CompletionItem> {
    vec![
        CompletionItem {
            label: "fn".into(),
            detail: "Function definition".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "pub fn".into(),
            detail: "Public function definition".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "val".into(),
            detail: "Immutable value binding".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "type".into(),
            detail: "Type definition".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "union".into(),
            detail: "Tagged union type".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "enum".into(),
            detail: "Enumeration type".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "extern".into(),
            detail: "External FFI declaration".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "mut".into(),
            detail: "Mutable modifier".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "ref".into(),
            detail: "Reference type".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "self".into(),
            detail: "Self parameter".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "if".into(),
            detail: "Conditional".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "elif".into(),
            detail: "Else-if branch".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "else".into(),
            detail: "Else branch".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "for".into(),
            detail: "Loop construct".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "in".into(),
            detail: "For-in loop".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "break".into(),
            detail: "Exit loop".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "continue".into(),
            detail: "Next iteration".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "return".into(),
            detail: "Return from function".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "match".into(),
            detail: "Pattern matching".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "when".into(),
            detail: "Conditional compilation".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "otherwise".into(),
            detail: "Match catch-all (legacy)".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "some".into(),
            detail: "Option some variant".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "none".into(),
            detail: "Option none variant".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "ok".into(),
            detail: "Result ok variant".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "err".into(),
            detail: "Result error variant".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "trust".into(),
            detail: "Call untrusted function from pure context".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "guarded".into(),
            detail: "Guarded precondition".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "pre".into(),
            detail: "Preconditions block".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "post".into(),
            detail: "Postconditions block".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "test".into(),
            detail: "Test block".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "assert".into(),
            detail: "Runtime assertion".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "void".into(),
            detail: "Unit type".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "bool".into(),
            detail: "Boolean type".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "true".into(),
            detail: "Boolean true".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "false".into(),
            detail: "Boolean false".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "use".into(),
            detail: "Import module".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "pub".into(),
            detail: "Public visibility".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "deref".into(),
            detail: "Dereference pointer".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "addr".into(),
            detail: "Take address".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "defer".into(),
            detail: "Deferred execution".into(),
            kind: lsp_types::CompletionItemKind::KEYWORD,
        },
        CompletionItem {
            label: "i32".into(),
            detail: "32-bit signed integer".into(),
            kind: lsp_types::CompletionItemKind::TYPE_PARAMETER,
        },
        CompletionItem {
            label: "i64".into(),
            detail: "64-bit signed integer".into(),
            kind: lsp_types::CompletionItemKind::TYPE_PARAMETER,
        },
        CompletionItem {
            label: "u8".into(),
            detail: "8-bit unsigned integer".into(),
            kind: lsp_types::CompletionItemKind::TYPE_PARAMETER,
        },
        CompletionItem {
            label: "usize".into(),
            detail: "Unsigned pointer-sized integer".into(),
            kind: lsp_types::CompletionItemKind::TYPE_PARAMETER,
        },
        CompletionItem {
            label: "f64".into(),
            detail: "64-bit float".into(),
            kind: lsp_types::CompletionItemKind::TYPE_PARAMETER,
        },
        CompletionItem {
            label: "char".into(),
            detail: "Character type".into(),
            kind: lsp_types::CompletionItemKind::TYPE_PARAMETER,
        },
        CompletionItem {
            label: "option[T]".into(),
            detail: "Optional type".into(),
            kind: lsp_types::CompletionItemKind::TYPE_PARAMETER,
        },
        CompletionItem {
            label: "result[T, E]".into(),
            detail: "Result type".into(),
            kind: lsp_types::CompletionItemKind::TYPE_PARAMETER,
        },
        CompletionItem {
            label: "list[T]".into(),
            detail: "Dynamic list/array".into(),
            kind: lsp_types::CompletionItemKind::TYPE_PARAMETER,
        },
        CompletionItem {
            label: "ref void".into(),
            detail: "Raw untyped pointer".into(),
            kind: lsp_types::CompletionItemKind::TYPE_PARAMETER,
        },
    ]
}

#[derive(Debug, Clone)]
pub struct CompletionItem {
    pub label: String,
    pub detail: String,
    pub kind: lsp_types::CompletionItemKind,
}
