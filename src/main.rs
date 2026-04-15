mod analysis;
mod ast;
mod lexer;
mod stdlib;

use analysis::*;
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread;

type Analyses = Arc<(Mutex<HashMap<String, Arc<DocumentAnalysis>>>, Condvar)>;

fn main() {
    std::panic::set_hook(Box::new(|info| {
        eprintln!("PANIC: {}", info);
        if let Some(loc) = info.location() {
            eprintln!("  at {}:{}:{}", loc.file(), loc.line(), loc.column());
        }
    }));

    eprintln!("[spectre-ls] starting...");

    stdlib::init_stdlib();
    if let Some(lib) = stdlib::get_stdlib() {
        eprintln!("[spectre-ls] stdlib loaded from: {:?}", lib.std_dir);
        eprintln!(
            "[spectre-ls] stdlib modules: {:?}",
            lib.modules.keys().collect::<Vec<_>>()
        );
    } else {
        eprintln!("[spectre-ls] warning: stdlib not found");
    }

    let result = run();
    match result {
        Ok(_) => eprintln!("[spectre-ls] exited normally"),
        Err(e) => eprintln!("[spectre-ls] fatal error: {}", e),
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (connection, ..) = lsp_server::Connection::stdio();

    eprintln!("[spectre-ls] stdio connected");

    let capabilities = lsp_types::ServerCapabilities {
        text_document_sync: Some(lsp_types::TextDocumentSyncCapability::Kind(
            lsp_types::TextDocumentSyncKind::FULL,
        )),
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        definition_provider: Some(lsp_types::OneOf::Left(true)),
        document_symbol_provider: Some(lsp_types::OneOf::Left(true)),
        signature_help_provider: Some(lsp_types::SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
            retrigger_characters: Some(vec![",".to_string()]),
            work_done_progress_options: Default::default(),
        }),
        completion_provider: Some(lsp_types::CompletionOptions {
            resolve_provider: Some(false),
            trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
            all_commit_characters: None,
            work_done_progress_options: Default::default(),
            completion_item: None,
        }),
        references_provider: Some(lsp_types::OneOf::Left(true)),
        ..Default::default()
    };

    let server_capabilities = serde_json::to_value(&capabilities)?;
    eprintln!("[spectre-ls] capabilities serialized");

    eprintln!("[spectre-ls] waiting for initialize...");
    let init_params = connection.initialize(server_capabilities)?;
    eprintln!("[spectre-ls] initialized, params: {:?}", init_params);

    let documents: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let analyses: Analyses = Arc::new((Mutex::new(HashMap::new()), Condvar::new()));
    let (an_tx, an_rx) = mpsc::channel::<(String, String)>();
    let analyses_worker = Arc::clone(&analyses);

    let worker_handle = thread::spawn(move || {
        for (uri, text) in an_rx.iter() {
            eprintln!("[spectre-ls] worker analyze start: {}", uri);
            let analysis = analyze(&text);
            eprintln!(
                "[spectre-ls] worker analyze done: {} (symbols={}, funcs={}, types={})",
                uri,
                analysis.symbols.len(),
                analysis.fn_by_name.len(),
                analysis.type_defs.len()
            );
            let (lock, cvar) = &*analyses_worker;
            {
                let mut a = lock.lock().unwrap();
                a.insert(uri, Arc::new(analysis));
            }
            cvar.notify_all();
        }
        eprintln!("[spectre-ls] analysis worker exiting");
    });

    eprintln!("[spectre-ls] entering message loop");

    let mut shutting_down = false;

    {
        let an_tx = an_tx;
        loop {
            let msg = match connection.receiver.recv() {
                Ok(msg) => msg,
                Err(_) => {
                    eprintln!("[spectre-ls] connection closed, exiting");
                    break;
                }
            };

            match msg {
                lsp_server::Message::Request(req) => {
                    eprintln!("[spectre-ls] request: {}", req.method);
                    if connection.handle_shutdown(&req)? {
                        eprintln!("[spectre-ls] shutdown request received, waiting for exit");
                        shutting_down = true;
                        continue;
                    }

                    if shutting_down {
                        eprintln!(
                            "[spectre-ls] ignoring request after shutdown: {}",
                            req.method
                        );
                        continue;
                    }
                    let sender = connection.sender.clone();
                    let documents_clone = Arc::clone(&documents);
                    let analyses_clone = Arc::clone(&analyses);
                    thread::spawn(move || {
                        match handle_request(&req, &documents_clone, &analyses_clone) {
                            Ok(Some(resp)) => {
                                let _ = sender.send(lsp_server::Message::Response(resp));
                            }
                            Ok(None) => {
                                eprintln!("[spectre-ls] no response for {}", req.method);
                            }
                            Err(e) => {
                                eprintln!("[spectre-ls] error handling {}: {}", req.method, e);
                                let resp = lsp_server::Response {
                                    id: req.id.clone(),
                                    result: None,
                                    error: Some(lsp_server::ResponseError {
                                        code: lsp_server::ErrorCode::InternalError as i32,
                                        message: e.to_string(),
                                        data: None,
                                    }),
                                };
                                let _ = sender.send(lsp_server::Message::Response(resp));
                            }
                        }
                    });
                }
                lsp_server::Message::Notification(notification) => {
                    eprintln!("[spectre-ls] notification: {}", notification.method);

                    if notification.method == "exit" {
                        eprintln!("[spectre-ls] exit notification received, exiting");
                        break;
                    }

                    if let Err(e) =
                        handle_notification(&notification, &documents, &analyses, &an_tx)
                    {
                        eprintln!("[spectre-ls] error handling notification: {}", e);
                    }
                }
                lsp_server::Message::Response(resp) => {
                    eprintln!("[spectre-ls] response: {:?}", resp);
                }
            }
        }
    }
    let _ = worker_handle.join();
    eprintln!("[spectre-ls] worker thread joined");
    Ok(())
}

fn handle_request(
    req: &lsp_server::Request,
    documents: &Arc<Mutex<HashMap<String, String>>>,
    analyses: &Analyses,
) -> Result<Option<lsp_server::Response>, Box<dyn std::error::Error>> {
    match req.method.as_str() {
        "textDocument/hover" => Ok(handle_hover(req, documents, analyses)),
        "textDocument/definition" => Ok(handle_definition(req, documents, analyses)),
        "textDocument/documentSymbol" => Ok(handle_document_symbol(req, documents, analyses)),
        "textDocument/signatureHelp" => Ok(handle_signature_help(req, documents, analyses)),
        "textDocument/completion" => Ok(handle_completion(req, documents, analyses)),
        "textDocument/references" => Ok(handle_references(req, documents, analyses)),
        _ => {
            eprintln!("[spectre-ls] unhandled request: {}", req.method);
            Ok(None)
        }
    }
}

fn handle_notification(
    notification: &lsp_server::Notification,
    documents: &Arc<Mutex<HashMap<String, String>>>,
    analyses: &Analyses,
    sender: &mpsc::Sender<(String, String)>,
) -> Result<(), Box<dyn std::error::Error>> {
    match notification.method.as_str() {
        "textDocument/didOpen" | "textDocument/didChange" => {
            let params: serde_json::Value = serde_json::from_value(notification.params.clone())?;
            let uri = params["textDocument"]["uri"]
                .as_str()
                .unwrap_or("")
                .to_string();

            let text = if notification.method == "textDocument/didOpen" {
                params["textDocument"]["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string()
            } else {
                params["contentChanges"][0]["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string()
            };

            eprintln!(
                "[spectre-ls] scheduling analysis: {} ({} bytes)",
                uri,
                text.len()
            );

            {
                let mut docs = documents.lock().unwrap();
                docs.insert(uri.clone(), text.clone());
            }

            // send to analysis worker (non-blocking send)
            let _ = sender.send((uri.clone(), text));
        }
        "textDocument/didClose" => {
            let params: serde_json::Value = serde_json::from_value(notification.params.clone())?;
            let uri = params["textDocument"]["uri"]
                .as_str()
                .unwrap_or("")
                .to_string();
            {
                let mut docs = documents.lock().unwrap();
                docs.remove(&uri);
            }
            {
                let (lock, _) = &**analyses;
                let mut a = lock.lock().unwrap();
                a.remove(&uri);
            }
        }
        "$/cancelRequest" | "$/setTrace" => {
            // ignore
        }
        _ => {
            eprintln!(
                "[spectre-ls] unhandled notification: {}",
                notification.method
            );
        }
    }
    Ok(())
}

fn get_analysis(
    uri: &str,
    documents: &Arc<Mutex<HashMap<String, String>>>,
    analyses: &Analyses,
) -> Option<Arc<DocumentAnalysis>> {
    eprintln!("[spectre-ls] [GET_ANALYSIS] looking for {}", uri);
    let (lock, cvar) = &**analyses;

    {
        let a = lock.lock().unwrap();
        eprintln!("[spectre-ls] [GET_ANALYSIS] cache has {} entries", a.len());
        if let Some(analysis) = a.get(uri) {
            eprintln!("[spectre-ls] [GET_ANALYSIS] cache hit for {}", uri);
            return Some(Arc::clone(analysis));
        }
    }

    eprintln!("[spectre-ls] [GET_ANALYSIS] cache miss, checking documents");
    let maybe_text = {
        let docs = documents.lock().unwrap();
        docs.get(uri).cloned()
    };

    let text = maybe_text?;

    if text.len() <= 100_000 {
        eprintln!(
            "[spectre-ls] [GET_ANALYSIS] performing synchronous analysis for {} ({} bytes)",
            uri,
            text.len()
        );
        let analysis = Arc::new(analyze(&text));
        eprintln!(
            "[spectre-ls] [GET_ANALYSIS] analysis complete: {} symbols",
            analysis.symbols.len()
        );
        let mut a = lock.lock().unwrap();
        a.insert(uri.to_string(), Arc::clone(&analysis));
        return Some(analysis);
    }

    eprintln!(
        "[spectre-ls] [GET_ANALYSIS] file too large ({} bytes), waiting for worker analysis",
        text.len()
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut a = lock.lock().unwrap();
    loop {
        if let Some(analysis) = a.get(uri) {
            eprintln!("[spectre-ls] [GET_ANALYSIS] worker analysis ready");
            return Some(Arc::clone(analysis));
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            eprintln!("[spectre-ls] [GET_ANALYSIS] timed out waiting for worker analysis");
            return None;
        }
        let (guard, timed_out) = cvar.wait_timeout(a, remaining).unwrap();
        a = guard;
        if timed_out.timed_out() {
            eprintln!("[spectre-ls] [GET_ANALYSIS] timed out waiting for worker analysis");
            return None;
        }
    }
}

fn lsp_position(source: &str, offset: usize) -> lsp_types::Position {
    let chars: Vec<char> = source.chars().collect();
    let mut line = 0u32;
    let mut col = 0u32;

    for (_i, &c) in chars.iter().enumerate() {
        if _i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }

    lsp_types::Position {
        line,
        character: col,
    }
}

fn offset_from_position(source: &str, position: lsp_types::Position) -> usize {
    let target_line = position.line as usize;
    let target_col = position.character as usize;

    let mut offset = 0;
    let mut current_line = 0;

    for line in source.lines() {
        if current_line == target_line {
            let mut col = 0;
            for ch in line.chars() {
                if col == target_col {
                    return offset;
                }
                offset += ch.len_utf8();
                col += 1;
            }
            return offset;
        }
        offset += line.len() + 1;
        current_line += 1;
    }

    source.len()
}

fn lsp_range_from_span(source: &str, span: &ast::Span) -> lsp_types::Range {
    let start = lsp_position(source, span.start);
    let end = lsp_position(source, span.end);
    lsp_types::Range { start, end }
}

fn handle_hover(
    req: &lsp_server::Request,
    documents: &Arc<Mutex<HashMap<String, String>>>,
    analyses: &Analyses,
) -> Option<lsp_server::Response> {
    eprintln!("[spectre-ls] [HOVER] ===== START =====");

    let params: lsp_types::HoverParams = serde_json::from_value(req.params.clone()).ok()?;
    let uri = params
        .text_document_position_params
        .text_document
        .uri
        .to_string();
    let position = params.text_document_position_params.position;

    eprintln!(
        "[spectre-ls] [HOVER] uri={}, line={}, char={}",
        uri, position.line, position.character
    );

    let source = {
        let docs = documents.lock().unwrap();
        docs.get(&uri)?.clone()
    };

    eprintln!(
        "[spectre-ls] [HOVER] document retrieved, size={} bytes",
        source.len()
    );

    let offset = offset_from_position(&source, position);

    eprintln!("[spectre-ls] [HOVER] converted to offset={}", offset);

    let analysis = get_analysis(&uri, documents, analyses)?;
    eprintln!(
        "[spectre-ls] [HOVER] analysis retrieved: {} symbols, {} fn_by_name, {} type_defs",
        analysis.symbols.len(),
        analysis.fn_by_name.len(),
        analysis.type_defs.len()
    );

    eprintln!("[spectre-ls] [HOVER] calling hover_at(offset={})", offset);
    let hover = hover_at(&analysis, offset, &source);

    eprintln!(
        "[spectre-ls] [HOVER] hover_at returned: {:?}",
        hover.as_ref().map(|h| (&h.signature, &h.documentation))
    );

    let result = if let Some(h) = hover {
        eprintln!("[spectre-ls] [HOVER] returning user-defined hover");
        Some(create_hover_response(h))
    } else if let Some(stdlib_hover) = get_stdlib_hover_at_position(&source, offset) {
        eprintln!(
            "[spectre-ls] [HOVER] returning stdlib hover: {:?}",
            (&stdlib_hover.signature, &stdlib_hover.documentation)
        );
        Some(create_hover_response(stdlib_hover))
    } else {
        let debug_contents = lsp_types::Hover {
            contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                kind: lsp_types::MarkupKind::Markdown,
                value: "debug: a symbol in this file".to_string(),
            }),
            range: None,
        };
        Some(debug_contents)
    };

    Some(lsp_server::Response {
        id: req.id.clone(),
        result: Some(serde_json::to_value(result).unwrap()),
        error: None,
    })
}

fn create_hover_response(h: HoverResult) -> lsp_types::Hover {
    let mut contents = String::new();
    if !h.signature.is_empty() {
        contents.push_str("```spectre\n");
        contents.push_str(&h.signature);
        contents.push_str("\n```\n\n");
    }
    if !h.documentation.is_empty() {
        contents.push_str(&h.documentation);
    }

    lsp_types::Hover {
        contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
            kind: lsp_types::MarkupKind::Markdown,
            value: contents,
        }),
        range: None,
    }
}

fn get_stdlib_hover_at_position(source: &str, offset: usize) -> Option<HoverResult> {
    let chars: Vec<char> = source.chars().collect();
    if offset > chars.len() {
        return None;
    }

    let word = extract_word_at(&chars, offset);
    if word.is_empty() {
        return None;
    }

    let (module_prefix, name) = if let Some(dot_pos) = find_last_dot_before_offset(&chars, offset) {
        let prefix: String = chars[..dot_pos].iter().collect();
        let name_start = dot_pos + 1;
        let name_end = offset.min(chars.len());
        let name: String = chars[name_start..name_end].iter().collect();
        (prefix, name)
    } else {
        (String::new(), word)
    };

    eprintln!(
        "[spectre-ls] stdlib hover check: module={:?} name={:?}",
        module_prefix, name
    );

    stdlib::get_stdlib_hover(&module_prefix, &name)
}

fn find_last_dot_before_offset(chars: &[char], offset: usize) -> Option<usize> {
    let check_until = offset.min(chars.len());
    for i in (0..check_until).rev() {
        if chars[i] == '.' {
            let after_dot = i + 1;
            if after_dot < chars.len()
                && (chars[after_dot].is_alphanumeric() || chars[after_dot] == '_')
            {
                let before_dot = if i > 0 { i - 1 } else { 0 };
                if i == 0 || chars[before_dot].is_alphanumeric() || chars[before_dot] == '_' {
                    return Some(i);
                }
            }
        }
    }
    None
}

fn extract_word_at(chars: &[char], offset: usize) -> String {
    if offset == 0 || offset > chars.len() {
        return String::new();
    }

    let pos = if offset == chars.len() {
        offset - 1
    } else {
        offset
    };

    let mut start = pos;
    while start > 0
        && (chars[start - 1].is_alphanumeric()
            || chars[start - 1] == '_'
            || chars[start - 1] == '.')
    {
        start -= 1;
    }

    let mut end = pos;
    while end < chars.len()
        && (chars[end].is_alphanumeric() || chars[end] == '_' || chars[end] == '.')
    {
        end += 1;
    }

    chars[start..end].iter().collect()
}

fn handle_definition(
    req: &lsp_server::Request,
    documents: &Arc<Mutex<HashMap<String, String>>>,
    analyses: &Analyses,
) -> Option<lsp_server::Response> {
    let params: lsp_types::GotoDefinitionParams =
        serde_json::from_value(req.params.clone()).ok()?;
    let uri = params
        .text_document_position_params
        .text_document
        .uri
        .to_string();
    let position = params.text_document_position_params.position;

    let source = {
        let docs = documents.lock().unwrap();
        docs.get(&uri)?.clone()
    };
    let offset = offset_from_position(&source, position);

    eprintln!(
        "[spectre-ls] goto definition at uri={} offset={}",
        uri, offset
    );

    let analysis = get_analysis(&uri, documents, analyses)?;

    let result = goto_definition(&analysis, offset).map(|span| {
        let range = lsp_range_from_span(&source, &span);
        lsp_types::GotoDefinitionResponse::Array(vec![lsp_types::Location {
            uri: params
                .text_document_position_params
                .text_document
                .uri
                .clone(),
            range,
        }])
    });

    Some(lsp_server::Response {
        id: req.id.clone(),
        result: Some(serde_json::to_value(result).unwrap()),
        error: None,
    })
}

fn handle_document_symbol(
    req: &lsp_server::Request,
    documents: &Arc<Mutex<HashMap<String, String>>>,
    analyses: &Analyses,
) -> Option<lsp_server::Response> {
    let params: lsp_types::DocumentSymbolParams =
        serde_json::from_value(req.params.clone()).ok()?;
    let uri = params.text_document.uri.to_string();

    eprintln!("[spectre-ls] document symbols for uri={}", uri);

    let source = {
        let docs = documents.lock().unwrap();
        docs.get(&uri)?.clone()
    };
    let analysis = get_analysis(&uri, documents, analyses)?;
    let symbols = document_symbols(&analysis);

    let result: Vec<lsp_types::DocumentSymbol> = symbols
        .into_iter()
        .map(|s| convert_document_symbol(&source, s))
        .collect();

    eprintln!("[spectre-ls] returning {} document symbols", result.len());

    Some(lsp_server::Response {
        id: req.id.clone(),
        result: Some(serde_json::to_value(result).unwrap()),
        error: None,
    })
}

fn convert_document_symbol(source: &str, s: DocumentSymbol) -> lsp_types::DocumentSymbol {
    let range = lsp_range_from_span(&source, &s.span);
    let selection_range = range;

    lsp_types::DocumentSymbol {
        name: s.name,
        detail: s.detail,
        kind: s.kind,
        tags: None,
        deprecated: Some(false),
        range,
        selection_range,
        children: if s.children.is_empty() {
            None
        } else {
            Some(
                s.children
                    .into_iter()
                    .map(|c| convert_document_symbol(&source, c))
                    .collect(),
            )
        },
    }
}

fn handle_signature_help(
    req: &lsp_server::Request,
    documents: &Arc<Mutex<HashMap<String, String>>>,
    analyses: &Analyses,
) -> Option<lsp_server::Response> {
    eprintln!("[spectre-ls] [SIGHELP] ===== START =====");

    let params: lsp_types::SignatureHelpParams = serde_json::from_value(req.params.clone()).ok()?;
    let uri = params
        .text_document_position_params
        .text_document
        .uri
        .to_string();
    let position = params.text_document_position_params.position;

    eprintln!(
        "[spectre-ls] [SIGHELP] uri={}, line={}, char={}",
        uri, position.line, position.character
    );

    let source = {
        let docs = documents.lock().unwrap();
        docs.get(&uri)?.clone()
    };

    eprintln!(
        "[spectre-ls] [SIGHELP] document retrieved, size={} bytes",
        source.len()
    );

    let offset = offset_from_position(&source, position);

    eprintln!("[spectre-ls] [SIGHELP] converted to offset={}", offset);

    let analysis = get_analysis(&uri, documents, analyses)?;
    eprintln!("[spectre-ls] [SIGHELP] analysis retrieved");
    let sig_help = signature_help_at(&analysis, offset, &source);

    eprintln!(
        "[spectre-ls] [SIGHELP] signature_help_at returned: {:?}",
        sig_help.as_ref().map(|s| (&s.label, &s.parameters))
    );

    let result = sig_help.map(|sh| create_signature_help_response(sh));

    let result = if result.is_some() {
        result
    } else if let Some(stdlib_sig) = get_stdlib_signature_help_at_position(&source, offset) {
        eprintln!(
            "[spectre-ls] stdlib signature help result: {:?}",
            stdlib_sig.label
        );
        Some(create_signature_help_response(stdlib_sig))
    } else {
        None
    };

    Some(lsp_server::Response {
        id: req.id.clone(),
        result: Some(serde_json::to_value(result).unwrap()),
        error: None,
    })
}

fn create_signature_help_response(sh: SignatureHelpResult) -> lsp_types::SignatureHelp {
    let sig_info = lsp_types::SignatureInformation {
        label: sh.label.clone(),
        documentation: Some(lsp_types::Documentation::MarkupContent(
            lsp_types::MarkupContent {
                kind: lsp_types::MarkupKind::Markdown,
                value: sh.documentation,
            },
        )),
        parameters: Some(
            sh.parameters
                .into_iter()
                .map(|p| lsp_types::ParameterInformation {
                    label: lsp_types::ParameterLabel::Simple(p.label.clone()),
                    documentation: if p.documentation.is_empty() {
                        None
                    } else {
                        Some(lsp_types::Documentation::MarkupContent(
                            lsp_types::MarkupContent {
                                kind: lsp_types::MarkupKind::Markdown,
                                value: p.documentation,
                            },
                        ))
                    },
                })
                .collect(),
        ),
        active_parameter: Some(sh.active_parameter as u32),
    };

    lsp_types::SignatureHelp {
        signatures: vec![sig_info],
        active_signature: None,
        active_parameter: Some(sh.active_parameter as u32),
    }
}

fn get_stdlib_signature_help_at_position(
    source: &str,
    offset: usize,
) -> Option<SignatureHelpResult> {
    let chars: Vec<char> = source.chars().collect();

    let (module_prefix, fn_name, active_param) = find_call_context_at_offset(&chars, offset)?;

    eprintln!(
        "[spectre-ls] stdlib signature help check: module={:?} fn={:?} param={}",
        module_prefix, fn_name, active_param
    );

    let mut sig = stdlib::get_stdlib_signature_help(&module_prefix, &fn_name)?;

    sig.active_parameter = active_param;
    Some(sig)
}

fn find_call_context_at_offset(chars: &[char], offset: usize) -> Option<(String, String, usize)> {
    let end = offset.min(chars.len());

    let mut depth_paren: i32 = 0;
    let mut depth_bracket: i32 = 0;
    let mut comma_count: usize = 0;

    for i in (0..end).rev() {
        match chars[i] {
            ')' => depth_paren += 1,
            '(' => {
                if depth_paren == 0 && depth_bracket == 0 {
                    if let Some((module_prefix, fn_name)) = extract_fn_name_before(chars, i) {
                        return Some((module_prefix, fn_name, comma_count));
                    }
                    comma_count = 0;
                } else {
                    depth_paren -= 1;
                }
            }
            ']' => depth_bracket += 1,
            '[' => {
                if depth_bracket > 0 {
                    depth_bracket -= 1;
                }
            }
            ',' if depth_paren == 0 && depth_bracket == 0 => {
                comma_count += 1;
            }
            _ => {}
        }
    }

    None
}

fn extract_fn_name_before(chars: &[char], paren_pos: usize) -> Option<(String, String)> {
    let mut end = paren_pos;
    while end > 0 && chars[end - 1].is_whitespace() {
        end -= 1;
    }

    let mut start = end;
    while start > 0
        && (chars[start - 1].is_alphanumeric()
            || chars[start - 1] == '_'
            || chars[start - 1] == '.')
    {
        start -= 1;
    }

    if start >= end {
        return None;
    }

    let fn_text: String = chars[start..end].iter().collect();

    let parts: Vec<&str> = fn_text.rsplitn(2, '.').collect();
    if parts.len() == 2 {
        Some((parts[1].to_string(), parts[0].to_string()))
    } else if parts.len() == 1 {
        Some((String::new(), parts[0].to_string()))
    } else {
        None
    }
}

fn handle_completion(
    req: &lsp_server::Request,
    documents: &Arc<Mutex<HashMap<String, String>>>,
    _analyses: &Analyses,
) -> Option<lsp_server::Response> {
    let params: lsp_types::CompletionParams = serde_json::from_value(req.params.clone()).ok()?;
    let uri = params.text_document_position.text_document.uri.to_string();
    let position = params.text_document_position.position;

    let source = {
        let docs = documents.lock().unwrap();
        docs.get(&uri)?.clone()
    };
    let offset = offset_from_position(&source, position);

    eprintln!("[spectre-ls] completion at uri={} offset={}", uri, offset);

    let (context_completions, stdlib_completions) = get_completions_for_position(&source, offset);

    let mut items: Vec<lsp_types::CompletionItem> = Vec::new();

    for c in context_completions {
        items.push(lsp_types::CompletionItem {
            label: c.label,
            kind: Some(c.kind),
            detail: Some(c.detail),
            documentation: None,
            ..Default::default()
        });
    }

    for c in stdlib_completions {
        items.push(lsp_types::CompletionItem {
            label: c.label,
            kind: Some(c.kind),
            detail: Some(c.detail),
            documentation: None,
            ..Default::default()
        });
    }

    Some(lsp_server::Response {
        id: req.id.clone(),
        result: Some(serde_json::to_value(items).unwrap()),
        error: None,
    })
}

fn get_completions_for_position(
    source: &str,
    offset: usize,
) -> (Vec<CompletionItem>, Vec<stdlib::CompletionItem>) {
    let chars: Vec<char> = source.chars().collect();

    let mut trigger_completions = Vec::new();
    let mut stdlib_completions = Vec::new();

    if let Some(use_completions) = get_use_completions(&chars, offset) {
        return (trigger_completions, use_completions);
    }

    let dot_pos = find_trigger_position(&chars, offset, '.');
    let paren_pos = find_trigger_position(&chars, offset, '(');

    if let Some(dot_idx) = dot_pos {
        let prefix = extract_completion_prefix(&chars, dot_idx);
        eprintln!("[spectre-ls] dot completion with prefix: {:?}", prefix);

        if !prefix.is_empty() {
            if let Some(std_completions) = stdlib::get_stdlib_completions(&prefix) {
                stdlib_completions = std_completions;
            }
        }

        let word_at_cursor = extract_word_at_position(&chars, offset);
        if !word_at_cursor.is_empty() && (prefix.is_empty() || prefix.ends_with(&word_at_cursor)) {
            let analysis_completions =
                get_analysis_completions_for_prefix(source, &word_at_cursor, offset);
            trigger_completions = analysis_completions;
        }
    } else if let Some(paren_idx) = paren_pos {
        let call_context = extract_call_context(&chars, paren_idx);
        eprintln!("[spectre-ls] paren completion for call: {:?}", call_context);

        if let Some((module_prefix, fn_name)) = call_context {
            if let Some(sig_help) = stdlib::get_stdlib_signature_help(&module_prefix, &fn_name) {
                stdlib_completions = vec![stdlib::CompletionItem {
                    label: fn_name.clone(),
                    detail: sig_help.label,
                    kind: lsp_types::CompletionItemKind::FUNCTION,
                }];
            }
        }
    } else {
        trigger_completions = completions();
    }

    (trigger_completions, stdlib_completions)
}

fn get_use_completions(chars: &[char], offset: usize) -> Option<Vec<stdlib::CompletionItem>> {
    let end = offset.min(chars.len());

    let mut paren_depth = 0;
    let mut found_use_open = false;
    let mut found_use_paren = false;
    let mut found_quote = false;
    let mut use_start = 0;

    for i in (0..end).rev() {
        match chars[i] {
            '"' => {
                if paren_depth == 0 && !found_quote {
                    found_quote = true;
                }
            }
            ')' => {
                paren_depth += 1;
            }
            '(' => {
                paren_depth -= 1;
                if paren_depth == 0 && !found_use_paren {
                    if i > 0 && chars.get(i - 1).map(|c| *c == 'e').unwrap_or(false) {
                        found_use_paren = true;
                        use_start = i.saturating_sub(4);
                        if i >= 4 {
                            let maybe_use = &chars[i - 4..i];
                            if maybe_use == ['u', 's', 'e', '('] {
                                found_use_open = true;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if !found_use_open || !found_quote {
        return None;
    }

    let quote_pos = chars[..end].iter().rposition(|&c| c == '"')?;

    let mut module_start = quote_pos + 1;
    while module_start < end && chars[module_start].is_whitespace() {
        module_start += 1;
    }

    let prefix: String = chars[module_start..end].iter().collect();
    eprintln!("[spectre-ls] use completion with prefix: {:?}", prefix);

    let stdlib = stdlib::get_stdlib()?;
    let mut items = Vec::new();

    if prefix.is_empty() {
        items.push(stdlib::CompletionItem {
            label: "std".to_string(),
            detail: "Standard library".to_string(),
            kind: lsp_types::CompletionItemKind::MODULE,
        });
    } else if prefix == "std" {
        items.push(stdlib::CompletionItem {
            label: "std".to_string(),
            detail: "Standard library".to_string(),
            kind: lsp_types::CompletionItemKind::MODULE,
        });
        for (name, _) in &stdlib.std_module.submodules {
            items.push(stdlib::CompletionItem {
                label: format!("std/{}", name),
                detail: format!("module: {}", name),
                kind: lsp_types::CompletionItemKind::MODULE,
            });
        }
    } else if let Some(submodule) = prefix.strip_prefix("std/") {
        let submodule_name = submodule.split('/').next().unwrap_or(submodule);
        if let Some(module) = stdlib.modules.get(submodule_name) {
            for (name, _) in &module.submodules {
                items.push(stdlib::CompletionItem {
                    label: format!("{}/{}", prefix, name),
                    detail: format!("submodule: {}", name),
                    kind: lsp_types::CompletionItemKind::MODULE,
                });
            }
        }
        if !items.is_empty() || submodule.is_empty() {
            for (name, _) in &stdlib.std_module.submodules {
                if name.starts_with(submodule) {
                    items.push(stdlib::CompletionItem {
                        label: format!("std/{}", name),
                        detail: format!("module: {}", name),
                        kind: lsp_types::CompletionItemKind::MODULE,
                    });
                }
            }
        }
    }

    Some(items)
}

fn find_trigger_position(chars: &[char], offset: usize, trigger: char) -> Option<usize> {
    if offset == 0 || offset > chars.len() {
        return None;
    }

    let check_pos = if offset == chars.len() {
        offset - 1
    } else {
        offset
    };

    for i in (0..=check_pos).rev() {
        let c = chars[i];
        if c == trigger {
            return Some(i);
        }
        if c.is_whitespace() || c == ')' || c == '(' || c == ';' || c == '{' || c == '}' {
            break;
        }
    }
    None
}

fn extract_completion_prefix(chars: &[char], dot_pos: usize) -> String {
    let mut start = dot_pos;
    while start > 0 && chars[start - 1].is_whitespace() {
        start -= 1;
    }

    let mut end = start;
    while end < chars.len()
        && (chars[end].is_alphanumeric() || chars[end] == '_' || chars[end] == '.')
    {
        end += 1;
    }

    chars[start..end].iter().collect()
}

fn extract_word_at_position(chars: &[char], offset: usize) -> String {
    if offset == 0 || offset > chars.len() {
        return String::new();
    }

    let pos = if offset == chars.len() {
        offset - 1
    } else {
        offset
    };

    let mut start = pos;
    while start > 0
        && (chars[start - 1].is_alphanumeric()
            || chars[start - 1] == '_'
            || chars[start - 1] == '.')
    {
        start -= 1;
    }

    let mut end = pos;
    while end < chars.len()
        && (chars[end].is_alphanumeric() || chars[end] == '_' || chars[end] == '.')
    {
        end += 1;
    }

    chars[start..end].iter().collect()
}

fn extract_call_context(chars: &[char], paren_pos: usize) -> Option<(String, String)> {
    let mut depth = 0i32;
    let mut i = paren_pos;

    while i > 0 {
        i -= 1;
        match chars[i] {
            ')' => depth += 1,
            '(' => {
                if depth == 0 {
                    let fn_end = i;
                    while i > 0 && chars[i - 1].is_whitespace() {
                        i -= 1;
                    }
                    let mut fn_start = i;
                    while fn_start > 0
                        && (chars[fn_start - 1].is_alphanumeric()
                            || chars[fn_start - 1] == '_'
                            || chars[fn_start - 1] == '.')
                    {
                        fn_start -= 1;
                    }

                    let fn_text: String = chars[fn_start..fn_end].iter().collect();

                    let parts: Vec<&str> = fn_text.rsplitn(2, '.').collect();
                    if parts.len() == 2 {
                        let fn_name = parts[0].to_string();
                        let module_prefix = parts[1].to_string();
                        return Some((module_prefix, fn_name));
                    } else if parts.len() == 1 {
                        return Some((String::new(), parts[0].to_string()));
                    }
                    return None;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

fn get_analysis_completions_for_prefix(
    source: &str,
    prefix: &str,
    _offset: usize,
) -> Vec<CompletionItem> {
    let analysis = crate::analysis::analyze(source);
    let mut items = Vec::new();

    for sym in &analysis.symbols {
        if sym.name.starts_with(prefix) || prefix.is_empty() {
            let kind = match sym.kind {
                crate::analysis::SymbolKind::Function => lsp_types::CompletionItemKind::FUNCTION,
                crate::analysis::SymbolKind::Type => lsp_types::CompletionItemKind::CLASS,
                crate::analysis::SymbolKind::Variable => lsp_types::CompletionItemKind::VARIABLE,
                crate::analysis::SymbolKind::Module => lsp_types::CompletionItemKind::MODULE,
                crate::analysis::SymbolKind::Constant => lsp_types::CompletionItemKind::CONSTANT,
                crate::analysis::SymbolKind::EnumVariant => {
                    lsp_types::CompletionItemKind::ENUM_MEMBER
                }
                crate::analysis::SymbolKind::Field => lsp_types::CompletionItemKind::FIELD,
                _ => lsp_types::CompletionItemKind::TEXT,
            };

            items.push(CompletionItem {
                label: sym.name.clone(),
                detail: sym.type_str.clone().unwrap_or_default(),
                kind,
            });
        }
    }

    items
}

fn handle_references(
    req: &lsp_server::Request,
    documents: &Arc<Mutex<HashMap<String, String>>>,
    analyses: &Analyses,
) -> Option<lsp_server::Response> {
    let params: lsp_types::ReferenceParams = serde_json::from_value(req.params.clone()).ok()?;
    let uri = params.text_document_position.text_document.uri.to_string();
    let position = params.text_document_position.position;

    let source = {
        let docs = documents.lock().unwrap();
        docs.get(&uri)?.clone()
    };
    let offset = offset_from_position(&source, position);

    let analysis = get_analysis(&uri, documents, analyses)?;

    let mut locations = Vec::new();
    for (span, _ctx) in &analysis.ident_spans {
        if offset >= span.start && offset < span.end {
            let range = lsp_range_from_span(&source, span);
            locations.push(lsp_types::Location {
                uri: params.text_document_position.text_document.uri.clone(),
                range,
            });
        }
    }

    if let Some(def_span) = goto_definition(&analysis, offset) {
        let range = lsp_range_from_span(&source, &def_span);
        locations.push(lsp_types::Location {
            uri: params.text_document_position.text_document.uri.clone(),
            range,
        });
    }

    Some(lsp_server::Response {
        id: req.id.clone(),
        result: Some(serde_json::to_value(locations).unwrap()),
        error: None,
    })
}
