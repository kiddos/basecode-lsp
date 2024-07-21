mod trie;

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use clap::Parser;
use glob::glob;
use simple_log::error;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService, Server};
use trie::Trie;

#[derive(Debug, Clone)]
struct Snippet {
    name: String,
    snippet: String,
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct LspArgs {
    #[arg(long)]
    snippet_folder: Option<String>,
}

#[derive(Debug)]
struct Backend {
    documents: Mutex<HashMap<String, String>>,
    snippets: Mutex<HashMap<String, Vec<Snippet>>>,
    trie: Mutex<Trie>,
    lsp_args: LspArgs,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        if let Some(snippet_folder) = self.lsp_args.snippet_folder.clone() {
            self.prepare_snippet(snippet_folder).await;
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    ..CompletionOptions::default()
                }),
                ..ServerCapabilities::default()
            },
            ..InitializeResult::default()
        })
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let mut document_lock = self.documents.lock().await; document_lock.insert(
            params.text_document.uri.to_string(),
            params.text_document.text.clone(),
        );

        self.add_words(params.text_document.text.clone()).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let mut document_lock = self.documents.lock().await;

        let uri = params.text_document.uri.to_string();
        if let Some(content) = document_lock.get(&uri) {
            self.remove_words(content.clone()).await;
        }
        document_lock.remove(&uri);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let mut document_lock = self.documents.lock().await;

        let uri = params.text_document.uri.to_string();
        if let Some(content) = document_lock.get_mut(&uri) {
            self.remove_words(content.clone()).await;
            if let Some(last_change) = params.content_changes.last() {
                *content = last_change.text.clone();
            }
        }
        for content_change in params.content_changes.iter() {
            self.add_words(content_change.text.clone()).await;
        }
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let prefix = self.get_prefix(&params).await;
        let trie_lock = self.trie.lock().await;
        let completion_words = trie_lock.suggest_completions(&prefix);
        let extension = get_file_extension(params.text_document_position.text_document.uri.to_string());
        let snippets = self.suggest_snippets(&extension, &prefix).await;

        let mut completions = Vec::new();
        completions.append(
            &mut completion_words
                .into_iter()
                .map(|word| CompletionItem {
                    label: word.clone(),
                    kind: Some(CompletionItemKind::TEXT),
                    sort_text: Some(word.clone()),
                    ..CompletionItem::default()
                })
                .collect(),
        );
        completions.append(
            &mut snippets
                .into_iter()
                .map(|snippet| CompletionItem {
                    label: snippet.name.clone(),
                    kind: Some(CompletionItemKind::SNIPPET),
                    documentation: Some(Documentation::String(snippet.snippet.clone())),
                    ..CompletionItem::default()
                })
                .collect(),
        );

        completions.sort_by_key(|item| item.label.clone());

        Ok(Some(CompletionResponse::Array(completions)))
    }
}

fn get_filename(path: String) -> String {
    let paths = path.split("/");
    match paths.last() {
        Some(filename) => match filename.find(".") {
            Some(index) => filename[0..index].to_string(),
            None => filename.to_string(),
        },
        None => String::new(),
    }
}

fn get_file_extension(path: String) -> String {
    let paths = path.split("/");
    match paths.last() {
        Some(filename) => match filename.rfind(".") {
            Some(index) => filename[index+1..].to_string(),
            None => filename.to_string(),
        },
        None => String::new(),
    }
}

fn snippet_extensions() -> &'static HashMap<&'static str, Vec<&'static str>> {
    static SNIPPETS: OnceLock<HashMap<&str, Vec<&str>>> = OnceLock::new();
    SNIPPETS.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert("c", vec!["c", "h"]);
        m.insert("cpp", vec!["cpp", "cc", "h", "hpp"]);
        m.insert("cmake", vec!["cmake"]);
        m.insert("dart", vec!["dart"]);
        m.insert("json", vec!["json"]);
        m.insert("python", vec!["python"]);
        m.insert("rust", vec!["rust", "rs", "rst"]);
        m.insert("sh", vec!["sh", "zsh"]);
        m.insert("zsh", vec!["sh", "zsh"]);
        m
    })
}

fn get_snippet_names(extension: &str) -> Vec<&str> {
    let mut names = Vec::new();
    for (name, extensions) in snippet_extensions().iter() {
        if extensions.contains(&extension) {
            names.push(*name);
        }
    }
    names
}

impl Backend {
    fn process_token(token: &str) -> Vec<String> {
        let mut cleaned = Vec::new();
        let mut current: Vec<char> = Vec::new();
        for ch in token.chars() {
            if !ch.is_alphanumeric() {
                if !current.is_empty() {
                    cleaned.push(current.iter().collect());
                    current = Vec::new();
                }
            } else {
                current.push(ch);
            }
        }

        if !current.is_empty() {
            cleaned.push(current.iter().collect());
        }
        cleaned
    }

    async fn add_words(&self, content: String) {
        let mut trie_lock = self.trie.lock().await;
        for token in content.split_whitespace() {
            let words = Self::process_token(token);
            for w in words {
                trie_lock.insert(&w);
            }
        }
    }

    async fn remove_words(&self, content: String) {
        let mut trie_lock = self.trie.lock().await;
        for token in content.split_whitespace() {
            let words = Self::process_token(token);
            for w in words {
                trie_lock.remove(&w);
            }
        }
    }

    async fn get_prefix(&self, params: &CompletionParams) -> String {
        let text_document_position = params.text_document_position.clone();
        let uri = text_document_position.text_document.uri.to_string();
        let document_lock = self.documents.lock().await;
        let position = text_document_position.position;
        let mut prefix = Vec::new();
        if let Some(content) = document_lock.get(&uri) {
            let current_line: Option<&str> = content.split("\n").nth(position.line as usize);
            if let Some(line) = current_line {
                let line: Vec<char> = line.chars().collect();
                let mut i = position.character as i32 - 1;
                while i >= 0 && (i as usize) < line.len() && line[i as usize].is_alphanumeric() {
                    prefix.push(line[i as usize]);
                    i -= 1;
                }
                prefix.reverse();
            }
        }
        prefix.iter().collect()
    }

    fn read_snippet(path: &Path) -> Vec<Snippet> {
        let mut snippets = Vec::new();
        if let Ok(content) = fs::read_to_string(path) {
            let lines: Vec<&str> = content.split("\n").collect();
            let mut i = 0;
            while i < lines.len() {
                let line = lines[i];
                if line.starts_with("snippet") {
                    let snippet_name = &line[("snippet".len() + 1)..];
                    i += 1;
                    let mut content_lines = Vec::new();
                    while i < lines.len() && !lines[i].starts_with("snippet") {
                        if !lines[i].starts_with("#") {
                            content_lines.push(lines[i].trim_start());
                        }
                        i += 1;
                    }
                    snippets.push(Snippet {
                        name: snippet_name.to_string(),
                        snippet: content_lines.join("\n").to_string(),
                    });
                } else {
                    i += 1;
                }
            }
        }
        snippets
    }

    async fn prepare_snippet(&self, snippet_path: String) {
        let target = format!("{}/*.snippets", snippet_path);
        if let Ok(paths) = glob(&target) {
            let mut snippet_lock = self.snippets.lock().await;
            for entry in paths {
                match entry {
                    Ok(path) => {
                        let p = path.as_path();
                        let filename = get_filename(p.display().to_string());
                        let all_snippets = Self::read_snippet(&p);
                        snippet_lock.insert(filename, all_snippets);
                    }
                    Err(e) => {
                        error!("{:?}", e);
                    }
                }
            }
        }
    }

    async fn suggest_snippets(&self, extension: &str, prefix: &str) -> Vec<Snippet> {
        let snippet_lock = self.snippets.lock().await;
        let snippet_names = get_snippet_names(extension);
        let mut result = Vec::new();
        for &snippet_name in snippet_names.iter() {
            if let Some(snippets) = snippet_lock.get(snippet_name) {
                for snippet in snippets.iter() {
                    if snippet.name.contains(prefix) {
                        result.push(snippet.clone());
                    }
                }
            }
        }
        result
    }
}

#[tokio::main]
async fn main() {
    let args = LspArgs::parse();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|_| Backend {
        documents: Mutex::new(HashMap::new()),
        snippets: Mutex::new(HashMap::new()),
        trie: Mutex::new(Trie::new()),
        lsp_args: args,
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
