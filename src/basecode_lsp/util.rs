use super::file::FileItem;
use super::snippet::Snippet;
use tower_lsp::lsp_types::*;

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

pub fn words_to_completion_items(
    words: Vec<String>,
    suffixes: &Vec<String>,
    completions: &mut Vec<CompletionItem>,
    kind: CompletionItemKind,
) {
    let items: Vec<CompletionItem> = words
        .iter()
        .filter(|&word| !suffixes.contains(word))
        .map(|word| CompletionItem {
            label: word.clone(),
            kind: Some(kind),
            sort_text: Some(word.clone()),
            ..CompletionItem::default()
        })
        .collect();
    completions.extend(items);
}

pub fn snippets_to_completion_items(snippets: Vec<Snippet>, completions: &mut Vec<CompletionItem>) {
    let items: Vec<CompletionItem> = snippets
        .into_iter()
        .map(|snippet| CompletionItem {
            label: snippet.name.clone(),
            kind: Some(CompletionItemKind::SNIPPET),
            documentation: Some(Documentation::String(snippet.markdown())),
            ..CompletionItem::default()
        })
        .collect();
    completions.extend(items);
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
