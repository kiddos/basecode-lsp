use glob::glob;
use simple_log::error;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct Snippet {
    pub name: String,
    pub snippet: String,
    pub filetype: String,
}

impl Snippet {
    pub fn markdown(&self) -> String {
        format!(
            "```{format}\n{snippet}```",
            format = self.filetype,
            snippet = self.snippet
        )
    }
}

pub fn snippet_patterns() -> &'static HashMap<&'static str, Vec<&'static str>> {
    static SNIPPETS: OnceLock<HashMap<&str, Vec<&str>>> = OnceLock::new();
    SNIPPETS.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert("c", vec![".c", ".h", ".cc", ".cpp"]);
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
            if file_uri.ends_with(*p) {
                names.push(*name);
                break;
            }
        }
    }
    names
}

fn transform_line(input: &str) -> String {
    let mut found_first_tab = false;
    let mut result = String::new();
    for ch in input.chars() {
        if ch == '\t' {
            if !found_first_tab {
                found_first_tab = true;
                continue;
            }
            result.push_str("  ");
        } else {
            result.push(ch);
        }
    }
    result
}

fn parse_snippets(content: &str, filetype: &str) -> Vec<Snippet> {
    let mut snippets = Vec::new();
    let lines: Vec<&str> = content.split("\n").collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("snippet") {
            let snippet_name = &line[("snippet".len() + 1)..];
            i += 1;
            let mut content_lines = Vec::new();
            while i < lines.len() && !lines[i].starts_with("snippet") {
                if !lines[i].starts_with("#") && lines[i].starts_with("\t") {
                    content_lines.push(transform_line(lines[i]));
                }
                i += 1;
            }
            snippets.push(Snippet {
                name: snippet_name.to_string(),
                snippet: content_lines.join("\n").to_string(),
                filetype: filetype.to_string(),
            });
        } else {
            i += 1;
        }
    }
    snippets
}

fn read_snippet(path: &Path, filetype: &str) -> Vec<Snippet> {
    if let Ok(content) = fs::read_to_string(path) {
        return parse_snippets(&content, filetype);
    }
    Vec::new()
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
                    let all_snippets = read_snippet(&p, &filename);
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

    #[test]
    fn test_parse_snippets() {
        let content = "snippet main
\tint main(void) {
\t\t// code here...
\t\treturn 0;
\t}";
        let snippets = parse_snippets(content, "cpp");
        assert_eq!(1, snippets.len());

        let snippet = &snippets[0];
        assert_eq!("main", snippet.name);
        assert_eq!(
            "int main(void) {
  // code here...
  return 0;
}",
            snippet.snippet
        );

        let content = "snippet for
\tfor (size_t i = 0; i < count; i++) {
\t\t/* code */
\t}";
        let snippets = parse_snippets(content, "cpp");
        assert_eq!(1, snippets.len());

        let snippet = &snippets[0];
        assert_eq!("for", snippet.name);
        assert_eq!(
            "for (size_t i = 0; i < count; i++) {
  /* code */
}",
            snippet.snippet
        );

        let content = "snippet if
\tif (condition) {
\t\t/* code */
\t}";
        let snippets = parse_snippets(content, "cpp");
        assert_eq!(1, snippets.len());

        let snippet = &snippets[0];
        assert_eq!("if", snippet.name);
        assert_eq!(
            "if (condition) {
  /* code */
}",
            snippet.snippet
        );
    }

    #[test]
    fn test_transform_line() {
        let transformed = transform_line("\t\treturn 0;");
        assert_eq!("  return 0;", transformed);
    }

    #[test]
    fn test_get_snippet_names() {
        let names = get_snippet_names("main.cpp");
        let expect_included = vec!["cpp", "c"];
        for t in expect_included.iter() {
            assert!(names.contains(t));
        }

        let names = get_snippet_names("main.cc");
        let expect_included = vec!["cpp", "c"];
        for t in expect_included.iter() {
            assert!(names.contains(t));
        }

        let names = get_snippet_names("main.cmake");
        assert!(names.contains(&"cmake"));

        let names = get_snippet_names("CMakeLists.txt");
        assert!(names.contains(&"cmake"));
    }

    #[test]
    fn test_snippet_patterns() {
        let patterns = snippet_patterns();
        assert!(patterns.contains_key("c"));
        assert!(patterns.contains_key("cpp"));
        assert!(patterns.get("rust").unwrap().contains(&".rs"));
    }
}
