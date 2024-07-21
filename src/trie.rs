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
