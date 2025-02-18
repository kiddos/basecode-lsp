use std::cmp::max;
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct TrieNode {
    children: HashMap<char, TrieNode>,
    word_count: i32,
}

#[derive(Debug)]
pub struct Trie {
    root: TrieNode,
}

impl Trie {
    pub fn new() -> Self {
        Trie {
            root: TrieNode::default(),
        }
    }

    pub fn insert(&mut self, word: &str) {
        let mut node = &mut self.root;
        for char in word.chars() {
            node = node.children.entry(char).or_default();
        }
        node.word_count += 1;
    }

    pub fn remove(&mut self, word: &str) {
        let w: Vec<char> = word.chars().collect();
        Self::remove_helper(&mut self.root, &w, 0);
    }

    fn remove_helper(node: &mut TrieNode, word: &Vec<char>, index: usize) -> bool {
        if index == word.len() {
            node.word_count = max(node.word_count - 1, 0);
            return node.children.is_empty() && node.word_count == 0;
        }

        let char = word[index];
        if let Some(child) = node.children.get_mut(&char) {
            let should_delete_child = Self::remove_helper(child, word, index + 1);
            if should_delete_child {
                node.children.remove(&char);
                return node.children.is_empty() && node.word_count == 0;
            }
        }
        false
    }

    pub fn suggest_completions(&self, prefix: &str) -> Vec<String> {
        let mut completions = Vec::new();
        let p: Vec<char> = prefix.chars().collect();
        self.suggest_completions_helper(&self.root, &p, 0, &mut completions);
        completions
    }

    fn suggest_completions_helper(
        &self,
        node: &TrieNode,
        prefix: &Vec<char>,
        index: usize,
        completions: &mut Vec<String>,
    ) {
        if index == prefix.len() {
            let mut current = prefix.clone();
            Self::collect_words(node, &mut current, completions);
            return;
        }

        if let Some(child) = node.children.get(&prefix[index]) {
            self.suggest_completions_helper(child, prefix, index + 1, completions);
        }
    }

    fn collect_words(node: &TrieNode, word: &mut Vec<char>, completions: &mut Vec<String>) {
        if node.word_count > 0 {
            completions.push(word.iter().collect());
        }

        for (&char, child) in node.children.iter() {
            word.push(char);
            Self::collect_words(child, word, completions);
            word.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trie_contains(trie: &Trie, word: &str) -> bool {
        let mut it = &trie.root;
        for ch in word.chars() {
            if let Some(node) = it.children.get(&ch) {
                it = &node;
            } else {
                return false;
            }
        }
        it.word_count > 0
    }

    #[test]
    fn test_insert_and_contains() {
        let mut trie = Trie::new();

        trie.insert("apple");
        trie.insert("application");
        trie.insert("banana");

        assert!(trie_contains(&trie, "apple"));
        assert!(trie_contains(&trie, "application"));
        assert!(trie_contains(&trie, "banana"));
        assert!(!trie_contains(&trie, "app"));
        assert!(!trie_contains(&trie, "ape"));
    }

    #[test]
    fn test_remove() {
        let mut trie = Trie::new();

        trie.insert("apple");
        trie.insert("application");
        trie.insert("banana");

        assert!(trie_contains(&trie, "apple"));
        trie.remove("apple");
        assert!(!trie_contains(&trie, "apple"));

        assert!(trie_contains(&trie, "application"));
        assert!(trie_contains(&trie, "banana"));
    }

    #[test]
    fn test_suggest_completions() {
        let mut trie = Trie::new();

        trie.insert("apple");
        trie.insert("application");
        trie.insert("banana");
        trie.insert("bat");
        trie.insert("bear");

        let mut completions = trie.suggest_completions("ap");
        completions.sort();
        assert_eq!(completions, vec!["apple", "application"]);

        let mut completions = trie.suggest_completions("ba");
        completions.sort();
        assert_eq!(completions, vec!["banana", "bat"]);

        let mut completions = trie.suggest_completions("b");
        completions.sort();
        assert_eq!(completions, vec!["banana", "bat", "bear"]);

        let completions = trie.suggest_completions("nonexistent");
        assert_eq!(completions, Vec::<String>::new());
    }

    #[test]
    fn test_remove_nonexistent() {
        let mut trie = Trie::new();

        trie.insert("apple");
        trie.remove("nonexistent");
        assert!(trie_contains(&trie, "apple"));
    }

    #[test]
    fn test_remove_multiple() {
        let mut trie = Trie::new();

        trie.insert("apple");
        trie.insert("apple");
        assert!(trie_contains(&trie, "apple"));

        trie.remove("apple");
        assert!(trie_contains(&trie, "apple"));
        trie.remove("apple");
        assert!(!trie_contains(&trie, "apple"));
    }
}
