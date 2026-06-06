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
use std::sync::Arc;
use tokio::task::JoinHandle;
use std::time::Duration;

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
    client: Client,
    documents: Arc<Mutex<HashMap<String, String>>>,
    snippets: Arc<Mutex<HashMap<String, Vec<Snippet>>>>,
    trie: Arc<Mutex<Trie>>,
    tmux_source: Arc<Mutex<Vec<String>>>,
    indexed_content: Arc<Mutex<HashMap<String, String>>>,
    debounce_tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    lsp_args: Arc<LspArgs>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        if let Some(snippet_folder) = self.lsp_args.snippet_folder.clone() {
            let message = format!("loading snippet folder: {}", snippet_folder);
            info!("{}", message);
            self.client.log_message(MessageType::INFO, message).await;
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
        let uri = params.text_document.uri.to_string();
        let text = params.text_document.text.clone();

        let mut document_lock = self.documents.lock().await;
        document_lock.insert(uri.clone(), text.clone());

        let mut indexed_lock = self.indexed_content.lock().await;
        indexed_lock.insert(uri.clone(), text.clone());

        self.add_words(text, uri).await;
        self.maybe_update_tmux().await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.to_string();

        let mut tasks_lock = self.debounce_tasks.lock().await;
        if let Some(task) = tasks_lock.remove(&uri) {
            task.abort();
        }

        let mut document_lock = self.documents.lock().await;
        let mut indexed_lock = self.indexed_content.lock().await;

        if let Some(content) = indexed_lock.remove(&uri) {
            self.remove_words(content).await;
        }
        document_lock.remove(&uri);
        self.maybe_update_tmux().await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.to_string();

        let mut document_lock = self.documents.lock().await;
        if let Some(content) = document_lock.get_mut(&uri) {
            if let Some(last_change) = params.content_changes.last() {
                *content = last_change.text.clone();
            }
        } else {
            if let Some(last_change) = params.content_changes.last() {
                document_lock.insert(uri.clone(), last_change.text.clone());
            }
        }
        drop(document_lock);

        let mut tasks_lock = self.debounce_tasks.lock().await;
        if let Some(task) = tasks_lock.remove(&uri) {
            task.abort();
        }

        let documents = Arc::clone(&self.documents);
        let indexed_content = Arc::clone(&self.indexed_content);
        let trie = Arc::clone(&self.trie);
        let tmux_source = Arc::clone(&self.tmux_source);
        let lsp_args = Arc::clone(&self.lsp_args);
        let client = self.client.clone();
        let uri_clone = uri.clone();

        let task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;

            let new_content = {
                let doc_lock = documents.lock().await;
                doc_lock.get(&uri_clone).cloned()
            };

            if let Some(new_content) = new_content {
                let mut indexed_lock = indexed_content.lock().await;
                let old_content = indexed_lock.get(&uri_clone).cloned();

                if let Some(old) = old_content {
                    remove_words_internal(Arc::clone(&trie), Arc::clone(&lsp_args), old).await;
                }

                add_words_internal(
                    Arc::clone(&trie),
                    client,
                    Arc::clone(&lsp_args),
                    new_content.clone(),
                    uri_clone.clone(),
                )
                .await;

                indexed_lock.insert(uri_clone, new_content);
                maybe_update_tmux_internal(lsp_args, tmux_source).await;
            }
        });

        tasks_lock.insert(uri, task);
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let text_document_position = params.text_document_position.clone();
        let position = text_document_position.position;

        let mut completions = Vec::new();
        if let Some(current_line) = self.get_current_line(&params).await {
            let prefix = get_word_prefix(&current_line, position.character as i32);
            if prefix.chars().next().map_or(false, |c| c.is_alphabetic()) {
                let trie_lock = self.trie.lock().await;
                let words = trie_lock.suggest_completions(&prefix);
                let suffixes = get_possible_current_word(&current_line, position.character as i32);
                words_uri_pair_to_completion_items(words, &suffixes, &mut completions, CompletionItemKind::TEXT);

                let tmux_words = self.prepare_tmux_words().await;
                let source = "tmux".to_string();
                words_to_completion_items(tmux_words, source, &suffixes, &mut completions, CompletionItemKind::REFERENCE);

                if self.lsp_args.command_source {
                    let mut command_words = get_command_completions();
                    command_words.sort();
                    command_words.dedup();
                    let source = "command".to_string();
                    words_to_completion_items(command_words, source, &suffixes, &mut completions, CompletionItemKind::KEYWORD);
                }

                let file_uri = params.text_document_position.text_document.uri.to_string();
                let snippets = self.suggest_snippets(&file_uri, &prefix).await;
                snippets_to_completion_items(snippets, &mut completions);

                if let Some(root_folder) = self.lsp_args.root_folder.clone() {
                    let file_items = get_file_items(&current_line, &root_folder);
                    file_items_to_completion_items(file_items, &params, &mut completions);
                }
            }
        }
        Ok(Some(CompletionResponse::Array(completions)))
    }
}

impl Backend {
    pub fn new(lsp_args: LspArgs, client: Client) -> Self {
        return Self {
            client,
            documents: Arc::new(Mutex::new(HashMap::new())),
            snippets: Arc::new(Mutex::new(HashMap::new())),
            trie: Arc::new(Mutex::new(Trie::new())),
            tmux_source: Arc::new(Mutex::new(Vec::new())),
            indexed_content: Arc::new(Mutex::new(HashMap::new())),
            debounce_tasks: Arc::new(Mutex::new(HashMap::new())),
            lsp_args: Arc::new(lsp_args),
        };
    }

    async fn add_words(&self, content: String, uri: String) {
        add_words_internal(
            Arc::clone(&self.trie),
            self.client.clone(),
            Arc::clone(&self.lsp_args),
            content,
            uri,
        )
        .await;
    }

    async fn remove_words(&self, content: String) {
        remove_words_internal(Arc::clone(&self.trie), Arc::clone(&self.lsp_args), content).await;
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
}

fn is_good_size(content: &str) -> bool {
    const MAX_CONTENT_SIZE: usize = 8 * 1024 * 1024;
    content.len() < MAX_CONTENT_SIZE
}

async fn add_words_internal(
    trie: Arc<Mutex<Trie>>,
    client: Client,
    lsp_args: Arc<LspArgs>,
    content: String,
    uri: String,
) {
    if !is_good_size(&content) {
        let message = format!("{} is too large, skip adding words", uri);
        client.log_message(MessageType::INFO, message).await;
        return;
    }

    let mut trie_lock = trie.lock().await;
    for token in content.split_whitespace() {
        let words = process_token(token, lsp_args.min_word_len);
        for w in words {
            trie_lock.insert(&w, &uri);
        }
    }
}

async fn remove_words_internal(trie: Arc<Mutex<Trie>>, lsp_args: Arc<LspArgs>, content: String) {
    if !is_good_size(&content) {
        return;
    }

    let mut trie_lock = trie.lock().await;
    for token in content.split_whitespace() {
        let words = process_token(token, lsp_args.min_word_len);
        for w in words {
            trie_lock.remove(&w);
        }
    }
}

async fn maybe_update_tmux_internal(
    lsp_args: Arc<LspArgs>,
    tmux_source: Arc<Mutex<Vec<String>>>,
) {
    if lsp_args.tmux_source {
        let tmux_content = retrieve_tmux_words(lsp_args.min_word_len);
        let mut data = tmux_source.lock().await;
        data.clear();
        data.extend(tmux_content);
    }
}

impl Backend {

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
