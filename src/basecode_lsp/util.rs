use super::file::FileItem;
use hashbrown::HashMap;
use tower_lsp::lsp_types::*;

/// Calculates word scores in document content based on proximity to cursor line and word frequency.
pub fn calculate_document_word_scores(
    document_content: &str,
    cursor_line: usize,
    min_len: usize,
) -> HashMap<String, f64> {
    let mut scores = HashMap::new();
    for (line_idx, line) in document_content.lines().enumerate() {
        let dist = (line_idx as i64 - cursor_line as i64).abs() as f64;
        let line_weight = 1.0 / (1.0 + dist);

        for token in line.split_whitespace() {
            let words = process_token(token, min_len);
            for w in words {
                *scores.entry(w).or_insert(0.0) += line_weight;
            }
        }
    }
    scores
}

/// Generates a zero-padded sort_text string so higher scores sort earlier lexicographically.
pub fn format_sort_text(score: f64, label: &str) -> String {
    let clamped_score = score.max(0.0);
    let score_int = (clamped_score * 1000.0) as u64;
    let rank = 1_000_000_000_u64.saturating_sub(score_int);
    format!("{:010}_{}", rank, label)
}

pub fn valid_token_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

pub fn is_token(current: &Vec<char>, min_len: usize) -> bool {
    if current.len() < min_len {
        return false;
    }
    if current.iter().all(|c| c.is_digit(10)) {
        return false;
    }
    if let Some(first_char) = current.iter().next() {
        if first_char.is_digit(10) {
            return false;
        }
    }
    true
}

pub fn process_token(token: &str, min_len: usize) -> Vec<String> {
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

pub fn get_word_prefix(current_line: &str, character: i32) -> String {
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

pub fn get_possible_current_word(current_line: &str, character: i32) -> Vec<String> {
    let mut possible: Vec<String> = Vec::new();
    let line: Vec<char> = current_line.chars().collect();
    let mut i = (character - 1).min(line.len() as i32 - 1);
    let mut current = Vec::new();
    while i >= 0 && valid_token_char(line[i as usize]) {
        i -= 1;
    }
    i += 1;
    while (i as usize) < line.len() && valid_token_char(line[i as usize]) {
        current.push(line[i as usize]);
        possible.push(current.iter().collect());
        i += 1;
    }

    possible
}


pub fn file_items_to_completion_items(
    file_items: Vec<FileItem>,
    params: &CompletionParams,
    completions: &mut Vec<CompletionItem>,
) {
    let position = &params.text_document_position.position;
    let line = position.line;
    let mut items: Vec<CompletionItem> = Vec::new();
    for file_item in file_items.iter() {
        let text_edit = TextEdit {
            new_text: file_item.filename.clone(),
            range: Range {
                start: Position {
                    line,
                    character: file_item.pos as u32 + 1,
                },
                end: Position {
                    line,
                    character: (file_item.pos + file_item.filename.len()) as u32,
                },
            },
        };
        let completion_item = CompletionItem {
            label: file_item.filename.clone(),
            kind: Some(if file_item.is_dir {
                CompletionItemKind::FOLDER
            } else {
                CompletionItemKind::FILE
            }),
            text_edit: Some(CompletionTextEdit::Edit(text_edit)),
            ..CompletionItem::default()
        };
        items.push(completion_item);
    }
    completions.extend(items);
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
    fn test_get_word_suffixes() {
        let suffixes = get_possible_current_word("   ios::sync", 8);
        assert_eq!(vec!["s", "sy", "syn", "sync"], suffixes);

        let suffixes = get_possible_current_word("   ios::sync", 10);
        assert_eq!(vec!["s", "sy", "syn", "sync"], suffixes);

        let suffixes = get_possible_current_word("   int best = numeric_limits<int>::max();", 14);
        assert_eq!(
            vec![
                "n",
                "nu",
                "num",
                "nume",
                "numer",
                "numeri",
                "numeric",
                "numeric_",
                "numeric_l",
                "numeric_li",
                "numeric_lim",
                "numeric_limi",
                "numeric_limit",
                "numeric_limits"
            ],
            suffixes
        );

        let suffixes = get_possible_current_word("   int best = numeric_limits<int>::max();", 35);
        assert_eq!(vec!["m", "ma", "max"], suffixes);
    }

    #[test]
    fn test_calculate_document_word_scores() {
        let doc = "foo bar\nbaz foo\nfoo";
        let scores = calculate_document_word_scores(doc, 2, 2);
        let score_foo_0 = 1.0 / 3.0; // line 0: dist 2
        let score_foo_1 = 1.0 / 2.0; // line 1: dist 1
        let score_foo_2 = 1.0 / 1.0; // line 2: dist 0
        let expected_foo = score_foo_0 + score_foo_1 + score_foo_2;

        let foo_score = scores.get("foo").cloned().unwrap_or(0.0);
        let bar_score = scores.get("bar").cloned().unwrap_or(0.0);
        let baz_score = scores.get("baz").cloned().unwrap_or(0.0);

        assert!((foo_score - expected_foo).abs() < 1e-6);
        assert!(foo_score > bar_score);
        assert!(baz_score > bar_score);
    }

    #[test]
    fn test_format_sort_text() {
        let st1 = format_sort_text(100.0, "apple");
        let st2 = format_sort_text(10.0, "banana");
        assert!(st1 < st2, "Higher score item should sort before lower score item");
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
    fn test_file_items_to_completion_items() {
        let file_items = vec![
            FileItem {
                filename: "foo.txt".to_string(),
                pos: 10,
                is_dir: false,
            },
            FileItem {
                filename: "src".to_string(),
                pos: 10,
                is_dir: true,
            },
        ];

        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse("file:///dummy.txt").unwrap(),
                },
                position: Position {
                    line: 2,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: PartialResultParams {
                partial_result_token: None,
            },
            context: None,
        };

        let mut completions = Vec::new();
        file_items_to_completion_items(file_items, &params, &mut completions);

        assert_eq!(completions.len(), 2);

        let comp1 = &completions[0];
        assert_eq!(comp1.label, "foo.txt");
        assert_eq!(comp1.kind, Some(CompletionItemKind::FILE));
        if let Some(CompletionTextEdit::Edit(edit)) = &comp1.text_edit {
            assert_eq!(edit.new_text, "foo.txt");
            assert_eq!(edit.range.start.line, 2);
            assert_eq!(edit.range.start.character, 11);
            assert_eq!(edit.range.end.line, 2);
            assert_eq!(edit.range.end.character, 17);
        } else {
            panic!("Expected Edit");
        }

        let comp2 = &completions[1];
        assert_eq!(comp2.label, "src");
        assert_eq!(comp2.kind, Some(CompletionItemKind::FOLDER));
        if let Some(CompletionTextEdit::Edit(edit)) = &comp2.text_edit {
            assert_eq!(edit.new_text, "src");
            assert_eq!(edit.range.start.line, 2);
            assert_eq!(edit.range.start.character, 11);
            assert_eq!(edit.range.end.line, 2);
            assert_eq!(edit.range.end.character, 13);
        } else {
            panic!("Expected Edit");
        }
    }
}
