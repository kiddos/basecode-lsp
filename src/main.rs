mod trie;

use std::env;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

use clap::Parser;
use glob::glob;
use simple_log::{error, info};
use simple_log::LogConfigBuilder;
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
            let completion_words = trie_lock.suggest_completions(&prefix);
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

            let file_uri = params.text_document_position.text_document.uri.to_string();
            let snippets = self.suggest_snippets(&file_uri, &prefix).await;
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

            if let Some(root_folder) = self.lsp_args.root_folder.clone() {
                let mut root = PathBuf::from(&root_folder);
                let file_prefix = get_file_path_prefix(&current_line, position.character as i32);
                root = root.join(&file_prefix);
                let file_items = list_all_file_items(&root);
                completions.append(
                    &mut file_items
                        .into_iter()
                        .map(|file_item| CompletionItem {
                            label: file_item.clone(),
                            kind: Some(CompletionItemKind::FILE),
                            ..CompletionItem::default()
                        })
                        .collect(),
                );
            }

            completions.sort_by_key(|item| item.label.clone());
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

fn snippet_patterns() -> &'static HashMap<&'static str, Vec<&'static str>> {
    static SNIPPETS: OnceLock<HashMap<&str, Vec<&str>>> = OnceLock::new();
    SNIPPETS.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert("c", vec![".c", ".h"]);
        m.insert("cpp", vec![".cpp", ".cc", ".h", ".hpp"]);
        m.insert("cmake", vec![".cmake", "CMakeLists.txt"]);
        m.insert("dart", vec![".dart"]);
        m.insert("json", vec![".json"]);
        m.insert("python", vec![".py", ".pyc"]);
        m.insert("rust", vec![".rs", ".rst"]);
        m.insert("sh", vec![".sh", ".zsh", ".bash"]);
        m.insert("zsh", vec![".sh", ".zsh"]);
        m
    })
}

fn get_snippet_names(file_uri: &str) -> Vec<&str> {
    let mut names = Vec::new();
    for (name, patterns) in snippet_patterns().iter() {
        for p in patterns.iter() {
            if file_uri.contains(*p) {
                names.push(*name);
                break;
            }
        }
    }
    names
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

fn valid_file_path_char(ch: char) -> bool {
    ch.is_alphanumeric()
        || ch == '_'
        || ch == '/'
        || ch == '-'
        || ch == '('
        || ch == ')'
        || ch == '.'
}

fn get_file_path_prefix(line: &str, character: i32) -> String {
    let mut prefix = Vec::new();
    let line: Vec<char> = line.chars().collect();
    let mut i = (character - 1).min(line.len() as i32 - 1);
    while i >= 0 && valid_file_path_char(line[i as usize]) {
        prefix.push(line[i as usize]);
        i -= 1;
    }
    prefix.reverse();
    prefix.iter().collect()
}

fn list_all_file_items(path: &Path) -> Vec<String> {
    let read_result = fs::read_dir(path);
    let mut result = Vec::new();
    if let Ok(entries) = read_result {
        for item in entries {
            if let Ok(entry) = item {
                let current = entry.path();
                let filename = current.file_name().unwrap_or(OsStr::new("")).to_str();
                if let Some(f) = filename {
                    result.push(f.to_string());
                }
            }
        }
    }
    result
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
        let config = LogConfigBuilder::builder()
            .path(log_path)
            .build();
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

    #[test]
    fn test_get_file_path_prefix() {
        let prefix = get_file_path_prefix("./some/path/to/here", 19);
        assert_eq!("./some/path/to/here", prefix);

        let prefix = get_file_path_prefix("  ./a/b/c/d/e.cc", 12);
        assert_eq!("./a/b/c/d/", prefix);
    }

    #[test]
    fn test_list_all_files() {
        let items = list_all_file_items(Path::new("./"));
        for item in items.iter() {
            println!("item = {}", item);
        }
        assert!(items.iter().any(|s| s == "src"));
        assert!(items.iter().any(|s| s == ".git"));
        assert!(items.iter().any(|s| s == "README.md"));
        assert!(items.iter().any(|s| s == ".gitignore"));
        assert!(items.iter().any(|s| s == "Cargo.toml"));
        assert!(items.iter().any(|s| s == "Cargo.lock"));

        let items = list_all_file_items(Path::new("./src"));
        for item in items.iter() {
            println!("item = {}", item);
        }
        assert!(items.iter().any(|s| s == "main.rs"));
        assert!(items.iter().any(|s| s == "trie.rs"));

        let items = list_all_file_items(Path::new("doesnt_exist"));
        assert_eq!(0, items.len());
    }
}
