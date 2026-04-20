use crate::analysis::{DocumentAnalysis, HoverResult, SignatureHelpResult, SignatureParam};
use crate::ast::{Parser, TypeDefKind};
use crate::lexer::Lexer;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

static STDLIB_DATA: OnceLock<Option<StdLib>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct StdLib {
    pub std_dir: PathBuf,
    pub modules: HashMap<String, StdLibModule>,
    pub std_module: StdLibModule,
}

#[derive(Debug, Clone)]
pub struct StdLibModule {
    pub name: String,
    pub path: PathBuf,
    pub functions: HashMap<String, FunctionInfo>,
    pub types: HashMap<String, TypeDefInfo>,
    pub submodules: HashMap<String, String>,
    pub analysis: Option<DocumentAnalysis>,
}

#[derive(Debug, Clone)]
pub struct FunctionInfo {
    pub name: String,
    pub full_name: String,
    pub params: Vec<ParamInfo>,
    pub return_type: String,
    pub returns_untrusted: bool,
    pub doc: String,
    pub self_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParamInfo {
    pub name: String,
    pub ty: String,
}

#[derive(Debug, Clone)]
pub struct TypeDefInfo {
    pub name: String,
    pub full_name: String,
    pub kind: String,
    pub fields: Vec<FieldInfo>,
    pub doc: String,
    pub size: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub name: String,
    pub ty: String,
    pub is_mut: bool,
}

pub fn get_stdlib() -> Option<&'static StdLib> {
    STDLIB_DATA.get().and_then(|o| o.as_ref())
}

pub fn init_stdlib() {
    if STDLIB_DATA.get().is_some() {
        return;
    }

    let stdlib = discover_and_load_stdlib();
    let _ = STDLIB_DATA.set(stdlib);
}

fn discover_and_load_stdlib() -> Option<StdLib> {
    let std_dir = find_std_dir()?;
    eprintln!("[spectre-ls] found std dir at: {:?}", std_dir);

    let mut modules = HashMap::new();
    let mut std_module = StdLibModule {
        name: "std".to_string(),
        path: std_dir.clone(),
        functions: HashMap::new(),
        types: HashMap::new(),
        submodules: HashMap::new(),
        analysis: None,
    };

    if let Ok(entries) = fs::read_dir(&std_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "sx").unwrap_or(false) {
                let module_name = path.file_stem().unwrap().to_string_lossy().to_string();
                if module_name == "std" {
                    continue;
                }
                if let Some(module) = load_stdlib_module(&path, &module_name, &mut std_module) {
                    std_module
                        .submodules
                        .insert(module_name.clone(), module_name.clone());
                    modules.insert(module_name, module);
                }
            }
        }
    }

    let std_main_path = std_dir.join("std.sx");
    if std_main_path.exists() {
        if let Some(std_analysis) = analyze_std_file(&std_main_path) {
            for sym in &std_analysis.symbols {
                if matches!(sym.kind, crate::analysis::SymbolKind::Module) {
                    std_module
                        .submodules
                        .insert(sym.name.clone(), sym.name.clone());
                }
            }
            std_module.analysis = Some(std_analysis);
        }
    }

    Some(StdLib {
        std_dir,
        modules,
        std_module,
    })
}

fn find_std_dir() -> Option<PathBuf> {
    let output = std::process::Command::new("whereis")
        .arg("-b")
        .arg("spectre")
        .output()
        .ok()?;

    let output_str = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = output_str.split_whitespace().collect();

    if parts.len() < 2 {
        eprintln!("[spectre-ls] could not find spectre binary");
        return None;
    }

    let binary_path = PathBuf::from(parts[1]);

    let spectre_dir = binary_path.parent()?;

    let local_std = PathBuf::from("std");
    if local_std.exists() && local_std.is_dir() {
        return Some(local_std);
    }

    let parent_std = spectre_dir.join("std");
    if parent_std.exists() && parent_std.is_dir() {
        return Some(parent_std);
    }

    let project_std = spectre_dir.join("share").join("spectre").join("std");
    if project_std.exists() && project_std.is_dir() {
        return Some(project_std);
    }

    None
}

fn load_stdlib_module(
    path: &PathBuf,
    name: &str,
    std_module: &mut StdLibModule,
) -> Option<StdLibModule> {
    let content = fs::read_to_string(path).ok()?;

    let mut lexer = Lexer::new(&content);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens, content.clone());
    let module = parser.parse_module();

    let mut functions = HashMap::new();
    let mut types = HashMap::new();

    for item in &module.items {
        match item {
            crate::ast::Item::Function(f) => {
                if !f.is_pub {
                    continue;
                }
                let params: Vec<ParamInfo> = f
                    .params
                    .iter()
                    .map(|p| ParamInfo {
                        name: p.name.clone(),
                        ty: p.ty.display(),
                    })
                    .collect();

                let return_type = if f.returns_untrusted {
                    format!("{}!", f.return_type.display())
                } else {
                    f.return_type.display()
                };

                let self_type = f.self_type.as_ref().map(|s| s.to_string());

                let full_name = if let Some(ref st) = self_type {
                    format!("({}).{}", st, f.name)
                } else {
                    f.name.clone()
                };

                let doc = f.doc_comments.join("\n");

                let info = FunctionInfo {
                    name: f.name.clone(),
                    full_name,
                    params,
                    return_type,
                    returns_untrusted: f.returns_untrusted,
                    doc,
                    self_type,
                };

                let lookup_name = if info.name.contains('(') {
                    info.name.clone()
                } else {
                    info.full_name.clone()
                };

                functions.insert(lookup_name, info);
            }
            crate::ast::Item::TypeDef(td) => {
                if !td.is_pub {
                    continue;
                }

                let fields: Vec<FieldInfo> = match &td.kind {
                    TypeDefKind::Struct(sf) => sf
                        .iter()
                        .map(|f| FieldInfo {
                            name: f.name.clone(),
                            ty: f.ty.display(),
                            is_mut: f.is_mut,
                        })
                        .collect(),
                    _ => Vec::new(),
                };

                let doc = td.doc_comments.join("\n");

                let kind_str = match &td.kind {
                    TypeDefKind::Struct(_) => "struct",
                    TypeDefKind::Union(_) => "union",
                    TypeDefKind::UnionConstruct(_) => "union (constructors)",
                    TypeDefKind::Enum(_) => "enum",
                };

                let info = TypeDefInfo {
                    name: td.name.clone(),
                    full_name: td.name.clone(),
                    kind: kind_str.to_string(),
                    fields,
                    doc,
                    size: None,
                };

                types.insert(td.name.clone(), info);
            }
            crate::ast::Item::Use(local_name, _, _) => {
                std_module
                    .submodules
                    .insert(local_name.clone(), local_name.clone());
            }
            _ => {}
        }
    }

    let analysis = analyze_std_file(path);

    Some(StdLibModule {
        name: name.to_string(),
        path: path.clone(),
        functions,
        types,
        submodules: HashMap::new(),
        analysis,
    })
}

fn analyze_std_file(path: &PathBuf) -> Option<DocumentAnalysis> {
    let content = fs::read_to_string(path).ok()?;
    Some(crate::analysis::analyze(&content))
}

pub fn get_stdlib_completions(prefix: &str) -> Option<Vec<CompletionItem>> {
    let stdlib = get_stdlib()?;
    let mut items = Vec::new();

    if prefix.is_empty() || prefix == "std" {
        for (name, _) in &stdlib.std_module.submodules {
            items.push(CompletionItem {
                label: name.clone(),
                detail: format!("module: {}", name),
                kind: lsp_types::CompletionItemKind::MODULE,
            });
        }
        for (name, func) in &stdlib.std_module.functions {
            items.push(CompletionItem {
                label: name.clone(),
                detail: format!("fn {}", func.full_name),
                kind: lsp_types::CompletionItemKind::FUNCTION,
            });
        }
    } else if let Some(module_name) = prefix.strip_prefix("std.") {
        let module_name = module_name.split('.').next().unwrap_or(module_name);
        if let Some(module) = stdlib.modules.get(module_name) {
            for (name, func) in &module.functions {
                items.push(CompletionItem {
                    label: name.clone(),
                    detail: format!("fn {}", func.full_name),
                    kind: lsp_types::CompletionItemKind::FUNCTION,
                });
            }
            for (name, ty) in &module.types {
                items.push(CompletionItem {
                    label: name.clone(),
                    detail: format!("{} {}", ty.kind, name),
                    kind: lsp_types::CompletionItemKind::CLASS,
                });
            }
            for (name, _) in &module.submodules {
                items.push(CompletionItem {
                    label: name.clone(),
                    detail: format!("module: {}", name),
                    kind: lsp_types::CompletionItemKind::MODULE,
                });
            }
        }
    }

    Some(items)
}

pub fn get_stdlib_hover(module_prefix: &str, name: &str) -> Option<HoverResult> {
    let stdlib = get_stdlib()?;

    if module_prefix.is_empty() || module_prefix == "std" {
        if let Some(func) = stdlib.std_module.functions.get(name) {
            return Some(create_function_hover(func));
        }
    }

    let module_name = module_prefix.strip_prefix("std.").unwrap_or(module_prefix);
    let module_name = module_name.split('.').next().unwrap_or(module_name);

    if let Some(module) = stdlib.modules.get(module_name) {
        if let Some(func) = module.functions.get(name) {
            return Some(create_function_hover(func));
        }
        if let Some(ty) = module.types.get(name) {
            return Some(create_type_hover(ty));
        }
    }

    None
}

pub fn get_stdlib_signature_help(
    module_prefix: &str,
    fn_name: &str,
) -> Option<SignatureHelpResult> {
    let stdlib = get_stdlib()?;

    let module_name = if module_prefix.is_empty() || module_prefix == "std" {
        None
    } else {
        let m = module_prefix.strip_prefix("std.").unwrap_or(module_prefix);
        Some(m.split('.').next().unwrap_or(m))
    };

    let module = match module_name {
        Some(name) => stdlib.modules.get(name)?,
        None => &stdlib.std_module,
    };

    let func = module.functions.get(fn_name)?;

    let label = format!(
        "fn {}({}) -> {}",
        func.full_name,
        func.params
            .iter()
            .map(|p| format!("{}: {}", p.name, p.ty))
            .collect::<Vec<_>>()
            .join(", "),
        func.return_type
    );

    let parameters: Vec<SignatureParam> = func
        .params
        .iter()
        .map(|p| SignatureParam {
            label: format!("{}: {}", p.name, p.ty),
            documentation: String::new(),
        })
        .collect();

    Some(SignatureHelpResult {
        label,
        parameters,
        active_parameter: 0,
        documentation: func.doc.clone(),
    })
}

fn create_function_hover(func: &FunctionInfo) -> HoverResult {
    let mut sig = format!(
        "fn {}({}) {}",
        func.full_name,
        func.params
            .iter()
            .map(|p| format!("{}: {}", p.name, p.ty))
            .collect::<Vec<_>>()
            .join(", "),
        func.return_type
    );

    if func.returns_untrusted {
        sig.push_str("\n\n(!) UNTRUSTED FUNCTION — may have unintended side-effects and bypasses the trust system.");
    }

    HoverResult {
        signature: sig,
        documentation: func.doc.clone(),
    }
}

fn create_type_hover(ty: &TypeDefInfo) -> HoverResult {
    let fields_str = ty
        .fields
        .iter()
        .map(|f| {
            let mut_s = if f.is_mut { "mut " } else { "" };
            format!("    {}{}: {}", mut_s, f.name, f.ty)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let signature = if fields_str.is_empty() {
        format!("{} {}", ty.kind, ty.name)
    } else {
        format!("{} {} {{\n{}\n}}", ty.kind, ty.name, fields_str)
    };

    let mut doc = ty.doc.clone();
    if let Some(size) = ty.size {
        if !doc.is_empty() {
            doc.push_str("\n\n");
        }
        doc.push_str(&format!("Size: {} bytes", size));
    }

    HoverResult {
        signature,
        documentation: doc,
    }
}

#[derive(Debug, Clone)]
pub struct CompletionItem {
    pub label: String,
    pub detail: String,
    pub kind: lsp_types::CompletionItemKind,
}
