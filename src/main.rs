mod file;
mod snippets;
mod trie;

use std::collections::HashMap;
use std::env;

use clap::Parser;
use file::get_file_items;
use simple_log::LogConfigBuilder;
use simple_log::{error, info};
use snippets::{get_snippet_names, prepare_snippet, Snippet};
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService, Server};
use trie::Trie;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct LspArgs {
    #[arg(long)]
    snippet_folder: Option<String>,
    #[arg(long)]
    root_folder: Option<String>,
    #[arg(long, default_value_t = 2)]
    min_word_len: usize,
    #[arg(long)]
    debug: bool,
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
            info!("loading snippet folder: {}", snippet_folder);
            let mut snippets_lock = self.snippets.lock().await;
            prepare_snippet(snippet_folder, &mut snippets_lock);
        }

        let trigger_characters = Some(vec!["/".to_string(), "\"".to_string(), "'".to_string()]);
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters,
                    ..CompletionOptions::default()
                }),
                ..ServerCapabilities::default()
            },
            ..InitializeResult::default()
        })
    }

    async fn shutdown(&self) -> Result<()> {
        info!("shutdown basecode-lsp");
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let mut document_lock = self.documents.lock().await;
        document_lock.insert(
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
        let text_document_position = params.text_document_position.clone();
        let position = text_document_position.position;

        let mut completions = Vec::new();
        if let Some(current_line) = self.get_current_line(&params).await {
            let prefix = get_word_prefix(&current_line, position.character as i32);

            let trie_lock = self.trie.lock().await;
            let words = trie_lock.suggest_completions(&prefix);
            words_to_completion_items(words, &prefix, &mut completions);

            let file_uri = params.text_document_position.text_document.uri.to_string();
            let snippets = self.suggest_snippets(&file_uri, &prefix).await;
            snippets_to_completion_items(snippets, &mut completions);

            if let Some(root_folder) = self.lsp_args.root_folder.clone() {
                let file_items = get_file_items(&current_line, &root_folder);
                file_items_to_completion_items(file_items, &mut completions);
            }
        }
        Ok(Some(CompletionResponse::Array(completions)))
    }
}

fn valid_token_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn is_token(current: &Vec<char>, min_len: usize) -> bool {
    if current.len() < min_len {
        return false;
    }
    if current.iter().all(|c| c.is_digit(10)) {
        return false;
    }
    true
}

fn process_token(token: &str, min_len: usize) -> Vec<String> {
    let mut cleaned = Vec::new();
    let mut current: Vec<char> = Vec::new();
    for ch in token.chars() {
        if !valid_token_char(ch) {
            if is_token(&current, min_len) {
                cleaned.push(current.iter().collect());
            }
            current.clear();
        } else {
            current.push(ch);
        }
    }

    if is_token(&current, min_len) {
        cleaned.push(current.iter().collect());
    }
    cleaned
}

fn get_word_prefix(current_line: &str, character: i32) -> String {
    let mut prefix = Vec::new();
    let line: Vec<char> = current_line.chars().collect();
    let mut i = (character - 1).min(line.len() as i32 - 1);
    while i >= 0 && valid_token_char(line[i as usize]) {
        prefix.push(line[i as usize]);
        i -= 1;
    }
    prefix.reverse();
    prefix.iter().collect()
}

fn words_to_completion_items(
    words: Vec<String>,
    prefix: &String,
    completions: &mut Vec<CompletionItem>,
) {
    let mut items: Vec<CompletionItem> = words
        .iter()
        .filter(|&word| word != prefix)
        .map(|word| CompletionItem {
            label: word.clone(),
            kind: Some(CompletionItemKind::TEXT),
            sort_text: Some(word.clone()),
            ..CompletionItem::default()
        })
        .collect();
    completions.append(&mut items);
}

fn snippets_to_completion_items(snippets: Vec<Snippet>, completions: &mut Vec<CompletionItem>) {
    let mut items: Vec<CompletionItem> = snippets
        .into_iter()
        .map(|snippet| CompletionItem {
            label: snippet.name.clone(),
            kind: Some(CompletionItemKind::SNIPPET),
            documentation: Some(Documentation::String(snippet.markdown())),
            ..CompletionItem::default()
        })
        .collect();
    completions.append(&mut items);
}

fn file_items_to_completion_items(file_items: Vec<String>, completions: &mut Vec<CompletionItem>) {
    let mut items: Vec<CompletionItem> = file_items
        .into_iter()
        .map(|file_item| CompletionItem {
            label: file_item.clone(),
            kind: Some(CompletionItemKind::FILE),
            ..CompletionItem::default()
        })
        .collect();
    completions.append(&mut items);
}

impl Backend {
    async fn add_words(&self, content: String) {
        let mut trie_lock = self.trie.lock().await;
        for token in content.split_whitespace() {
            let words = process_token(token, self.lsp_args.min_word_len);
            for w in words {
                trie_lock.insert(&w);
            }
        }
    }

    async fn remove_words(&self, content: String) {
        let mut trie_lock = self.trie.lock().await;
        for token in content.split_whitespace() {
            let words = process_token(token, self.lsp_args.min_word_len);
            for w in words {
                trie_lock.remove(&w);
            }
        }
    }

    async fn get_current_line(&self, params: &CompletionParams) -> Option<String> {
        let text_document_position = params.text_document_position.clone();
        let uri = text_document_position.text_document.uri.to_string();
        let document_lock = self.documents.lock().await;
        let position = text_document_position.position;
        if let Some(content) = document_lock.get(&uri) {
            let current_line: Option<&str> = content.split("\n").nth(position.line as usize);
            if let Some(line) = current_line {
                return Some(line.to_string());
            }
        }
        None
    }

    async fn suggest_snippets(&self, file_uri: &str, prefix: &str) -> Vec<Snippet> {
        let snippet_lock = self.snippets.lock().await;
        let snippet_names = get_snippet_names(file_uri);
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

fn setup_debug_logging() {
    let mut temp_dir = env::temp_dir();
    temp_dir.push("baselsp.log");
    if let Some(log_path) = temp_dir.to_str() {
        let config = LogConfigBuilder::builder().path(log_path).build();
        if let Err(_e) = simple_log::new(config) {
            error!("fail to setup log {}", log_path);
            return;
        }
    }
}

#[tokio::main]
async fn main() {
    let args = LspArgs::parse();

    if args.debug {
        setup_debug_logging();
    }

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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_get_word_prefix() {
        let prefix = get_word_prefix("   ios::sync_with_stdio", 24);
        assert_eq!("sync_with_stdio", prefix);

        let prefix = get_word_prefix("   int best = numeric_limits<int>::max();", 28);
        assert_eq!("numeric_limits", prefix);

        let prefix = get_word_prefix("   int best = numeric_limits<int>::max();", 38);
        assert_eq!("max", prefix);
    }

    #[test]
    fn test_process_token() {
        let tokens = process_token("   aho_corasick(root.get())", 2);
        assert_eq!(vec!["aho_corasick", "root", "get"], tokens);

        let tokens = process_token(
            "   vector<int> solve(string s, vector<int>& k, vector<string>& m)",
            2,
        );
        assert_eq!(
            vec!["vector", "int", "solve", "string", "vector", "int", "vector", "string"],
            tokens
        );

        let tokens = process_token("   TrieNode* tn = node->failure", 3);
        assert_eq!(vec!["TrieNode", "node", "failure"], tokens);
    }
}
