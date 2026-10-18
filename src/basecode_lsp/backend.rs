use super::file::*;
use super::snippet::*;
use super::tmux::*;
use super::command::*;
use super::trie::*;
use super::util::*;

use clap::Parser;
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use simple_log::*;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::*;
use std::sync::Arc;
use tokio::task::JoinHandle;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum StringOrVec {
    Single(String),
    Vec(Vec<String>),
}

impl StringOrVec {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            StringOrVec::Single(s) => vec![s],
            StringOrVec::Vec(v) => v,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum SnippetFoldersParams {
    SingleFolder(String),
    VecFolders(Vec<String>),
    FolderObject {
        #[serde(alias = "folders", alias = "snippet_folders", alias = "snippet_folder", alias = "folder_path", alias = "folder")]
        folder_paths: StringOrVec,
    },
}

impl SnippetFoldersParams {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            SnippetFoldersParams::SingleFolder(s) => vec![s],
            SnippetFoldersParams::VecFolders(v) => v,
            SnippetFoldersParams::FolderObject { folder_paths } => folder_paths.into_vec(),
        }
    }
}

#[derive(Debug)]
pub struct Candidate {
    pub kind: CompletionItemKind,
    pub word: String,
    pub count: i32,
    pub source: String,
    pub documentation: Option<String>,
    pub text_edit: Option<CompletionTextEdit>,
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct LspArgs {
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
    documents: Arc<RwLock<HashMap<String, String>>>,
    snippets: Arc<RwLock<HashMap<String, Vec<Snippet>>>>,
    trie: Arc<RwLock<Trie>>,
    tmux_source: Arc<RwLock<Vec<String>>>,
    indexed_content: Arc<RwLock<HashMap<String, String>>>,
    debounce_tasks: Arc<RwLock<HashMap<String, JoinHandle<()>>>>,
    lsp_args: Arc<LspArgs>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
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

        let mut document_lock = self.documents.write().await;
        document_lock.insert(uri.clone(), text.clone());

        let mut indexed_lock = self.indexed_content.write().await;
        indexed_lock.insert(uri.clone(), text.clone());

        self.add_words(text, uri).await;
        self.maybe_update_tmux().await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.to_string();

        let mut tasks_lock = self.debounce_tasks.write().await;
        if let Some(task) = tasks_lock.remove(&uri) {
            task.abort();
        }

        let mut document_lock = self.documents.write().await;
        let mut indexed_lock = self.indexed_content.write().await;

        if let Some(content) = indexed_lock.remove(&uri) {
            self.remove_words(content).await;
        }
        document_lock.remove(&uri);
        self.maybe_update_tmux().await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.to_string();

        let mut document_lock = self.documents.write().await;
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

        let mut tasks_lock = self.debounce_tasks.write().await;
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
                let doc_lock = documents.read().await;
                doc_lock.get(&uri_clone).cloned()
            };

            if let Some(new_content) = new_content {
                let mut indexed_lock = indexed_content.write().await;
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
        let file_uri = text_document_position.text_document.uri.to_string();

        let mut completions = Vec::new();
        if let Some(current_line) = self.get_current_line(&params).await {
            let prefix = get_word_prefix(&current_line, position.character as i32);
            if prefix.chars().next().map_or(false, |c| c.is_alphabetic()) {
                let suffixes = get_possible_current_word(&current_line, position.character as i32);

                let doc_content = {
                    let doc_lock = self.documents.read().await;
                    doc_lock.get(&file_uri).cloned().unwrap_or_default()
                };

                let doc_scores = calculate_document_word_scores(
                    &doc_content,
                    position.line as usize,
                    self.lsp_args.min_word_len,
                );

                let mut candidates: Vec<Candidate> = Vec::new();

                candidates.extend(self.get_trie_candidates(&prefix).await);
                candidates.extend(self.get_tmux_candidates(&prefix).await);
                candidates.extend(self.get_command_candidates(&prefix));
                candidates.extend(self.get_snippet_candidates(&file_uri, &prefix).await);
                candidates.extend(self.get_file_candidates(&current_line, &params));

                let mut merged_candidates: HashMap<String, Candidate> = HashMap::new();
                for cand in candidates {
                    if suffixes.contains(&cand.word) {
                        continue;
                    }
                    match merged_candidates.get_mut(&cand.word) {
                        Some(existing) => {
                            existing.count += cand.count;
                            if existing.kind == CompletionItemKind::TEXT && cand.kind != CompletionItemKind::TEXT {
                                existing.kind = cand.kind;
                            }
                            if existing.documentation.is_none() {
                                existing.documentation = cand.documentation;
                            }
                            if existing.text_edit.is_none() {
                                existing.text_edit = cand.text_edit;
                            }
                        }
                        None => {
                            merged_candidates.insert(cand.word.clone(), cand);
                        }
                    }
                }

                for (word, cand) in merged_candidates {
                    let doc_score = doc_scores.get(&word).cloned().unwrap_or(0.0);
                    let source_bonus = match cand.source.as_str() {
                        "snippet" => 100.0,
                        "file" => 50.0,
                        "tmux" => 0.5,
                        "command" => 0.1,
                        _ => 0.0,
                    };
                    let score = doc_score * 10.0 + (cand.count as f64) * 1.0 + source_bonus;

                    completions.push(CompletionItem {
                        label: word.clone(),
                        kind: Some(cand.kind),
                        sort_text: Some(format_sort_text(score, &word)),
                        detail: Some(cand.source),
                        documentation: cand.documentation.map(Documentation::String),
                        text_edit: cand.text_edit,
                        ..CompletionItem::default()
                    });
                }

                completions.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
            }
        }
        Ok(Some(CompletionResponse::Array(completions)))
    }
}

impl Backend {
    pub fn new(lsp_args: LspArgs, client: Client) -> Self {
        return Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
            snippets: Arc::new(RwLock::new(HashMap::new())),
            trie: Arc::new(RwLock::new(Trie::new())),
            tmux_source: Arc::new(RwLock::new(Vec::new())),
            indexed_content: Arc::new(RwLock::new(HashMap::new())),
            debounce_tasks: Arc::new(RwLock::new(HashMap::new())),
            lsp_args: Arc::new(lsp_args),
        };
    }

    pub async fn set_snippet_folders(&self, params: SnippetFoldersParams) {
        let folders = params.into_vec();
        let message = format!("loading snippet folders: {:?}", folders);
        info!("{}", message);
        self.client.log_message(MessageType::INFO, message).await;
        let mut snippets_lock = self.snippets.write().await;
        prepare_snippets(&folders, &mut snippets_lock);
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
        let document_lock = self.documents.read().await;
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
    trie: Arc<RwLock<Trie>>,
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

    let mut trie_lock = trie.write().await;
    for token in content.split_whitespace() {
        let words = process_token(token, lsp_args.min_word_len);
        for w in words {
            trie_lock.insert(&w, &uri);
        }
    }
}

async fn remove_words_internal(trie: Arc<RwLock<Trie>>, lsp_args: Arc<LspArgs>, content: String) {
    if !is_good_size(&content) {
        return;
    }

    let mut trie_lock = trie.write().await;
    for token in content.split_whitespace() {
        let words = process_token(token, lsp_args.min_word_len);
        for w in words {
            trie_lock.remove(&w);
        }
    }
}

async fn maybe_update_tmux_internal(
    lsp_args: Arc<LspArgs>,
    tmux_source: Arc<RwLock<Vec<String>>>,
) {
    if lsp_args.tmux_source {
        let tmux_content = retrieve_tmux_words(lsp_args.min_word_len).await;
        let mut data = tmux_source.write().await;
        data.clear();
        data.extend(tmux_content);
    }
}

impl Backend {
    async fn get_trie_candidates(&self, prefix: &str) -> Vec<Candidate> {
        let trie_lock = self.trie.read().await;
        let words = trie_lock.suggest_completions(prefix);
        words
            .into_iter()
            .map(|(word, uri, count)| Candidate {
                kind: CompletionItemKind::TEXT,
                word,
                count,
                source: uri,
                documentation: None,
                text_edit: None,
            })
            .collect()
    }

    async fn get_tmux_candidates(&self, prefix: &str) -> Vec<Candidate> {
        let tmux_words = self.prepare_tmux_words().await;
        tmux_words
            .into_iter()
            .filter(|w| w.starts_with(prefix))
            .map(|word| Candidate {
                kind: CompletionItemKind::REFERENCE,
                word,
                count: 1,
                source: "tmux".to_string(),
                documentation: None,
                text_edit: None,
            })
            .collect()
    }

    fn get_command_candidates(&self, prefix: &str) -> Vec<Candidate> {
        if !self.lsp_args.command_source {
            return Vec::new();
        }
        let command_words = get_command_completions();
        command_words
            .into_iter()
            .filter(|w| w.starts_with(prefix))
            .map(|word| Candidate {
                kind: CompletionItemKind::KEYWORD,
                word,
                count: 1,
                source: "command".to_string(),
                documentation: None,
                text_edit: None,
            })
            .collect()
    }

    async fn get_snippet_candidates(&self, file_uri: &str, prefix: &str) -> Vec<Candidate> {
        let snippets = self.suggest_snippets(file_uri, prefix).await;
        snippets
            .into_iter()
            .map(|snippet| Candidate {
                kind: CompletionItemKind::SNIPPET,
                word: snippet.name.clone(),
                count: 1,
                source: "snippet".to_string(),
                documentation: Some(snippet.markdown()),
                text_edit: None,
            })
            .collect()
    }

    fn get_file_candidates(&self, current_line: &str, params: &CompletionParams) -> Vec<Candidate> {
        let mut candidates = Vec::new();
        if let Some(root_folder) = self.lsp_args.root_folder.clone() {
            let file_items = get_file_items(current_line, &root_folder);
            let mut file_completions = Vec::new();
            file_items_to_completion_items(file_items, params, &mut file_completions);
            for item in file_completions {
                candidates.push(Candidate {
                    kind: item.kind.unwrap_or(CompletionItemKind::FILE),
                    word: item.label,
                    count: 1,
                    source: "file".to_string(),
                    documentation: None,
                    text_edit: item.text_edit,
                });
            }
        }
        candidates
    }

    async fn suggest_snippets(&self, file_uri: &str, prefix: &str) -> Vec<Snippet> {
        let snippet_lock = self.snippets.read().await;
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
            let tmux_content = retrieve_tmux_words(self.lsp_args.min_word_len).await;
            let mut data = self.tmux_source.write().await;
            data.clear();
            data.extend(tmux_content);
        }
    }

    async fn prepare_tmux_words(&self) -> Vec<String> {
        let data = self.tmux_source.read().await;
        let mut output = Vec::new();
        for word in data.iter() {
            output.push(word.clone());
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_completion_scoring_proximity_and_frequency() {
        let args = LspArgs {
            root_folder: None,
            min_word_len: 2,
            tmux_source: false,
            command_source: false,
            debug: false,
        };
        let (service, _socket) = LspService::new(|client| Backend::new(args, client));
        let backend = service.inner();

        let doc_uri = Url::parse("file:///test.rs").unwrap();
        // Line 0: apple_far
        // Line 1..10: empty lines
        // Line 11: apple_close
        // Line 12: app
        let content = "apple_far\n\n\n\n\n\n\n\n\n\n\napple_close\napp";

        backend.did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: doc_uri.clone(),
                language_id: "rust".to_string(),
                version: 1,
                text: content.to_string(),
            },
        }).await;

        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: doc_uri.clone() },
                position: Position { line: 12, character: 3 }, // On "app" at line 12
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        };

        let response = backend.completion(params).await.unwrap().unwrap();
        if let CompletionResponse::Array(items) = response {
            assert!(items.len() >= 2);
            // apple_close is on line 11 (1 line away), apple_far is on line 0 (12 lines away)
            let labels: Vec<String> = items.into_iter().map(|item| item.label).collect();
            assert_eq!(labels[0], "apple_close");
            assert_eq!(labels[1], "apple_far");
        } else {
            panic!("Expected CompletionResponse::Array");
        }
    }

    #[test]
    fn test_snippet_folders_params_deserialization() {
        // Test single folder string
        let val = json!("/path/to/snippets");
        let params: SnippetFoldersParams = serde_json::from_value(val).unwrap();
        assert_eq!(params.into_vec(), vec!["/path/to/snippets"]);

        // Test array of strings
        let val = json!(["/path/1", "/path/2"]);
        let params: SnippetFoldersParams = serde_json::from_value(val).unwrap();
        assert_eq!(params.into_vec(), vec!["/path/1", "/path/2"]);

        // Test object with folder_paths
        let val = json!({"folder_paths": ["/path/1", "/path/2"]});
        let params: SnippetFoldersParams = serde_json::from_value(val).unwrap();
        assert_eq!(params.into_vec(), vec!["/path/1", "/path/2"]);

        // Test object with folders / snippet_folders
        let val = json!({"folders": ["/path/a", "/path/b"]});
        let params: SnippetFoldersParams = serde_json::from_value(val).unwrap();
        assert_eq!(params.into_vec(), vec!["/path/a", "/path/b"]);

        // Test object with single folder_path / snippet_folder
        let val = json!({"folder_path": "/path/single"});
        let params: SnippetFoldersParams = serde_json::from_value(val).unwrap();
        assert_eq!(params.into_vec(), vec!["/path/single"]);
    }
}
