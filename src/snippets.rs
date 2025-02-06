use glob::glob;
use std::fs;
use simple_log::error;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct Snippet {
    pub name: String,
    pub snippet: String,
}

pub fn snippet_patterns() -> &'static HashMap<&'static str, Vec<&'static str>> {
    static SNIPPETS: OnceLock<HashMap<&str, Vec<&str>>> = OnceLock::new();
    SNIPPETS.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert("c", vec![".c", ".h"]);
        m.insert("cpp", vec![".cpp", ".cc", ".h", ".hpp"]);
        m.insert("cmake", vec![".cmake", "CMakeLists.txt"]);
        m.insert("dart", vec![".dart"]);
        m.insert("javascript", vec![".js", ".ts"]);
        m.insert("json", vec![".json"]);
        m.insert("kotlin", vec![".kt"]);
        m.insert("python", vec![".py", ".pyc"]);
        m.insert("rust", vec![".rs", ".rst"]);
        m.insert("sh", vec![".sh", ".zsh", ".bash"]);
        m.insert("zsh", vec![".sh", ".zsh"]);
        m.insert("clangformat", vec![".clang-format"]);
        m.insert("yapf", vec![".style.yapf"]);
        m
    })
}

pub fn get_snippet_names(file_uri: &str) -> Vec<&str> {
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

fn get_file_basename(path: String) -> String {
    let paths = path.split("/");
    match paths.last() {
        Some(filename) => match filename.find(".") {
            Some(index) => filename[0..index].to_string(),
            None => filename.to_string(),
        },
        None => String::new(),
    }
}

pub fn prepare_snippet(snippet_path: String, snippets: &mut HashMap<String, Vec<Snippet>>) {
    let target = format!("{}/*.snippets", snippet_path);
    if let Ok(paths) = glob(&target) {
        for entry in paths {
            match entry {
                Ok(path) => {
                    let p = path.as_path();
                    let filename = get_file_basename(p.display().to_string());
                    let all_snippets = read_snippet(&p);
                    snippets.insert(filename, all_snippets);
                }
                Err(e) => {
                    error!("{:?}", e);
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_get_file_basename() {
        let filename = get_file_basename("a/b/c/d.txt".to_string());
        assert_eq!("d", filename);

        let filename = get_file_basename("102938 !#@#! abcdqweio.txt".to_string());
        assert_eq!("102938 !#@#! abcdqweio", filename);
    }
}
