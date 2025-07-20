use super::file::*;
use super::snippet::*;
use super::tmux::*;
use super::command::*;
use super::trie::*;
use super::util::*;

use clap::Parser;
use hashbrown::HashMap;
use simple_log::*;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::*;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct LspArgs {
    #[arg(long)]
    snippet_folder: Option<String>,
    #[arg(long)]
    root_folder: Option<String>,
    #[arg(long, default_value_t = 2)]
    min_word_len: usize,
    #[arg(long, default_value_t = true)]
    tmux_source: bool,
    #[arg(long, default_value_t = false)]
    command_source: bool,
    #[arg(long)]
    pub debug: bool,
}

#[derive(Debug)]
pub struct Backend {
    documents: Mutex<HashMap<String, String>>,
    snippets: Mutex<HashMap<String, Vec<Snippet>>>,
    trie: Mutex<Trie>,
    tmux_source: Mutex<Vec<String>>,
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
        self.maybe_update_tmux().await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let mut document_lock = self.documents.lock().await;

        let uri = params.text_document.uri.to_string();
        if let Some(content) = document_lock.get(&uri) {
            self.remove_words(content.clone()).await;
        }
        document_lock.remove(&uri);
        self.maybe_update_tmux().await;
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
        self.maybe_update_tmux().await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let text_document_position = params.text_document_position.clone();
        let position = text_document_position.position;

        let mut completions = Vec::new();
        if let Some(current_line) = self.get_current_line(&params).await {
            let prefix = get_word_prefix(&current_line, position.character as i32);

            let trie_lock = self.trie.lock().await;
            let words = trie_lock.suggest_completions(&prefix);
            let suffixes = get_possible_current_word(&current_line, position.character as i32);
            words_to_completion_items(words, &suffixes, &mut completions, CompletionItemKind::TEXT);

            let tmux_words = self.prepare_tmux_words().await;
            words_to_completion_items(tmux_words, &suffixes, &mut completions, CompletionItemKind::REFERENCE);

            if self.lsp_args.command_source {
                let mut command_words = get_command_completions();
                command_words.sort();
                command_words.dedup();
                words_to_completion_items(command_words, &suffixes, &mut completions, CompletionItemKind::KEYWORD);
            }

            let file_uri = params.text_document_position.text_document.uri.to_string();
            let snippets = self.suggest_snippets(&file_uri, &prefix).await;
            snippets_to_completion_items(snippets, &mut completions);

            if let Some(root_folder) = self.lsp_args.root_folder.clone() {
                let file_items = get_file_items(&current_line, &root_folder);
                file_items_to_completion_items(file_items, &params, &mut completions);
            }
        }
        Ok(Some(CompletionResponse::Array(completions)))
    }
}

impl Backend {
    pub fn new(lsp_args: LspArgs) -> Self {
        return Self {
            documents: Mutex::new(HashMap::new()),
            snippets: Mutex::new(HashMap::new()),
            trie: Mutex::new(Trie::new()),
            tmux_source: Mutex::new(Vec::new()),
            lsp_args,
        };
    }

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

    async fn maybe_update_tmux(&self) {
        if self.lsp_args.tmux_source {
            let tmux_content = retrieve_tmux_words(self.lsp_args.min_word_len);
            let mut data = self.tmux_source.lock().await;
            data.clear();
            data.extend(tmux_content);
        }
    }

    async fn prepare_tmux_words(&self) -> Vec<String> {
        let data = self.tmux_source.lock().await;
        let mut output = Vec::new();
        for word in data.iter() {
            output.push(word.clone());
        }
        output
    }
}
