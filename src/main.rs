mod analysis;
mod ast;
mod lexer;

use analysis::*;
use std::collections::HashMap;

fn main() {
    std::panic::set_hook(Box::new(|info| {
        eprintln!("PANIC: {}", info);
        if let Some(loc) = info.location() {
            eprintln!("  at {}:{}:{}", loc.file(), loc.line(), loc.column());
        }
    }));

    eprintln!("[spectre-ls] starting...");

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

    let mut documents: HashMap<String, String> = HashMap::new();
    let mut analyses: HashMap<String, DocumentAnalysis> = HashMap::new();

    eprintln!("[spectre-ls] entering message loop");

    loop {
        let msg = match connection
            .receiver
            .recv_timeout(std::time::Duration::from_millis(100))
        {
            Ok(msg) => msg,
            Err(_) => continue,
        };

        match msg {
            lsp_server::Message::Request(req) => {
                eprintln!("[spectre-ls] request: {}", req.method);
                if connection.handle_shutdown(&req)? {
                    eprintln!("[spectre-ls] shutting down");
                    return Ok(());
                }
                match handle_request(&req, &documents, &mut analyses) {
                    Ok(Some(resp)) => {
                        connection
                            .sender
                            .send(lsp_server::Message::Response(resp))?;
                    }
                    Ok(None) => {
                        eprintln!("[spectre-ls] no response for {}", req.method);
                    }
                    Err(e) => {
                        eprintln!("[spectre-ls] error handling {}: {}", req.method, e);
                        // Send error response
                        let resp = lsp_server::Response {
                            id: req.id.clone(),
                            result: None,
                            error: Some(lsp_server::ResponseError {
                                code: lsp_server::ErrorCode::InternalError as i32,
                                message: e.to_string(),
                                data: None,
                            }),
                        };
                        connection
                            .sender
                            .send(lsp_server::Message::Response(resp))?;
                    }
                }
            }
            lsp_server::Message::Notification(notification) => {
                eprintln!("[spectre-ls] notification: {}", notification.method);
                if let Err(e) = handle_notification(&notification, &mut documents, &mut analyses) {
                    eprintln!("[spectre-ls] error handling notification: {}", e);
                }
            }
            lsp_server::Message::Response(resp) => {
                eprintln!("[spectre-ls] response: {:?}", resp);
            }
        }
    }
}

fn handle_request(
    req: &lsp_server::Request,
    documents: &HashMap<String, String>,
    analyses: &mut HashMap<String, DocumentAnalysis>,
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
    documents: &mut HashMap<String, String>,
    analyses: &mut HashMap<String, DocumentAnalysis>,
) -> Result<(), Box<dyn std::error::Error>> {
    match notification.method.as_str() {
        "textDocument/didOpen" | "textDocument/didChange" => {
            let params: serde_json::Value = serde_json::from_value(notification.params.clone())?;
            let uri = if notification.method == "textDocument/didOpen" {
                params["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string()
            } else {
                params["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string()
            };

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
                "[spectre-ls] analyzing document: {} ({} bytes)",
                uri,
                text.len()
            );
            let analysis = analyze(&text);
            eprintln!(
                "[spectre-ls] found {} symbols, {} functions, {} types",
                analysis.symbols.len(),
                analysis.fn_by_name.len(),
                analysis.type_defs.len()
            );
            documents.insert(uri.clone(), text);
            analyses.insert(uri.clone(), analysis);
        }
        "textDocument/didClose" => {
            let params: serde_json::Value = serde_json::from_value(notification.params.clone())?;
            let uri = params["textDocument"]["uri"]
                .as_str()
                .unwrap_or("")
                .to_string();
            documents.remove(&uri);
            analyses.remove(&uri);
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
    documents: &HashMap<String, String>,
    analyses: &mut HashMap<String, DocumentAnalysis>,
) -> Option<DocumentAnalysis> {
    if let Some(analysis) = analyses.get(uri) {
        return Some(analysis.clone());
    }
    if let Some(text) = documents.get(uri) {
        let analysis = analyze(text);
        analyses.insert(uri.to_string(), analysis.clone());
        return Some(analysis);
    }
    eprintln!("[spectre-ls] no analysis available for {}", uri);
    None
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
    let chars: Vec<char> = source.chars().collect();
    let mut line = 0u32;
    let mut col = 0u32;

    for (i, &c) in chars.iter().enumerate() {
        if line == position.line && col == position.character {
            return i;
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }

    chars.len()
}

fn lsp_range_from_span(source: &str, span: &ast::Span) -> lsp_types::Range {
    let start = lsp_position(source, span.start);
    let end = lsp_position(source, span.end);
    lsp_types::Range { start, end }
}

fn handle_hover(
    req: &lsp_server::Request,
    documents: &HashMap<String, String>,
    analyses: &mut HashMap<String, DocumentAnalysis>,
) -> Option<lsp_server::Response> {
    let params: lsp_types::HoverParams = serde_json::from_value(req.params.clone()).ok()?;
    let uri = params
        .text_document_position_params
        .text_document
        .uri
        .to_string();
    let position = params.text_document_position_params.position;

    let source = documents.get(&uri)?;
    let offset = offset_from_position(source, position);

    eprintln!("[spectre-ls] hover at uri={} offset={}", uri, offset);

    let analysis = get_analysis(&uri, documents, analyses)?;
    let hover = hover_at(&analysis, offset, source);

    eprintln!(
        "[spectre-ls] hover result: {:?}",
        hover.as_ref().map(|h| &h.signature)
    );

    let result = hover.map(|h| {
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
    });

    Some(lsp_server::Response {
        id: req.id.clone(),
        result: Some(serde_json::to_value(result).unwrap()),
        error: None,
    })
}

fn handle_definition(
    req: &lsp_server::Request,
    documents: &HashMap<String, String>,
    analyses: &mut HashMap<String, DocumentAnalysis>,
) -> Option<lsp_server::Response> {
    let params: lsp_types::GotoDefinitionParams =
        serde_json::from_value(req.params.clone()).ok()?;
    let uri = params
        .text_document_position_params
        .text_document
        .uri
        .to_string();
    let position = params.text_document_position_params.position;

    let source = documents.get(&uri)?;
    let offset = offset_from_position(source, position);

    eprintln!(
        "[spectre-ls] goto definition at uri={} offset={}",
        uri, offset
    );

    let analysis = get_analysis(&uri, documents, analyses)?;

    let result = goto_definition(&analysis, offset).map(|span| {
        let range = lsp_range_from_span(source, &span);
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
    documents: &HashMap<String, String>,
    analyses: &mut HashMap<String, DocumentAnalysis>,
) -> Option<lsp_server::Response> {
    let params: lsp_types::DocumentSymbolParams =
        serde_json::from_value(req.params.clone()).ok()?;
    let uri = params.text_document.uri.to_string();

    eprintln!("[spectre-ls] document symbols for uri={}", uri);

    let source = documents.get(&uri)?;
    let analysis = get_analysis(&uri, documents, analyses)?;
    let symbols = document_symbols(&analysis);

    let result: Vec<lsp_types::DocumentSymbol> = symbols
        .into_iter()
        .map(|s| convert_document_symbol(source, s))
        .collect();

    eprintln!("[spectre-ls] returning {} document symbols", result.len());

    Some(lsp_server::Response {
        id: req.id.clone(),
        result: Some(serde_json::to_value(result).unwrap()),
        error: None,
    })
}

fn convert_document_symbol(source: &str, s: DocumentSymbol) -> lsp_types::DocumentSymbol {
    let range = lsp_range_from_span(source, &s.span);
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
                    .map(|c| convert_document_symbol(source, c))
                    .collect(),
            )
        },
    }
}

fn handle_signature_help(
    req: &lsp_server::Request,
    documents: &HashMap<String, String>,
    analyses: &mut HashMap<String, DocumentAnalysis>,
) -> Option<lsp_server::Response> {
    let params: lsp_types::SignatureHelpParams = serde_json::from_value(req.params.clone()).ok()?;
    let uri = params
        .text_document_position_params
        .text_document
        .uri
        .to_string();
    let position = params.text_document_position_params.position;

    let source = documents.get(&uri)?;
    let offset = offset_from_position(source, position);

    eprintln!(
        "[spectre-ls] signature help at uri={} offset={}",
        uri, offset
    );

    let analysis = get_analysis(&uri, documents, analyses)?;
    let sig_help = signature_help_at(&analysis, offset);

    eprintln!(
        "[spectre-ls] signature help result: {:?}",
        sig_help.as_ref().map(|s| &s.label)
    );

    let result = sig_help.map(|sh| {
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
    });

    Some(lsp_server::Response {
        id: req.id.clone(),
        result: Some(serde_json::to_value(result).unwrap()),
        error: None,
    })
}

fn handle_completion(
    req: &lsp_server::Request,
    _documents: &HashMap<String, String>,
    _analyses: &mut HashMap<String, DocumentAnalysis>,
) -> Option<lsp_server::Response> {
    eprintln!("[spectre-ls] completion request");

    let items: Vec<lsp_types::CompletionItem> = completions()
        .into_iter()
        .map(|c| lsp_types::CompletionItem {
            label: c.label,
            kind: Some(c.kind),
            detail: Some(c.detail),
            documentation: None,
            ..Default::default()
        })
        .collect();

    Some(lsp_server::Response {
        id: req.id.clone(),
        result: Some(serde_json::to_value(items).unwrap()),
        error: None,
    })
}

fn handle_references(
    req: &lsp_server::Request,
    documents: &HashMap<String, String>,
    analyses: &mut HashMap<String, DocumentAnalysis>,
) -> Option<lsp_server::Response> {
    let params: lsp_types::ReferenceParams = serde_json::from_value(req.params.clone()).ok()?;
    let uri = params.text_document_position.text_document.uri.to_string();
    let position = params.text_document_position.position;

    let source = documents.get(&uri)?;
    let offset = offset_from_position(source, position);

    let analysis = get_analysis(&uri, documents, analyses)?;

    let mut locations = Vec::new();
    for (span, _ctx) in &analysis.ident_spans {
        if offset >= span.start && offset < span.end {
            let range = lsp_range_from_span(source, span);
            locations.push(lsp_types::Location {
                uri: params.text_document_position.text_document.uri.clone(),
                range,
            });
        }
    }

    if let Some(def_span) = goto_definition(&analysis, offset) {
        let range = lsp_range_from_span(source, &def_span);
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
