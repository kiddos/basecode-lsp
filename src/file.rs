use std::ffi::OsStr;
use std::fs;
use std::path::Path;

pub fn get_file_path_prefix(line: &str, character: i32) -> String {
    let mut prefix = Vec::new();
    let line: Vec<char> = line.chars().collect();
    let mut i = (character - 1).min(line.len() as i32 - 1);
    let mut start = -1;
    let mut found_sep = false;
    while i >= 0 {
        let c = line[i as usize];
        if c == '/' {
            found_sep = true;
        }
        if c == '/' || c == '.' {
            start = i;
        }
        prefix.push(c);
        i -= 1;
    }
    if start < 0 || !found_sep {
        return String::new();
    }
    prefix.reverse();
    prefix[start as usize..].iter().collect()
}

pub fn list_all_file_items(path: &Path) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_file_path_prefix() {
        let prefix = get_file_path_prefix("./a", 3);
        assert_eq!("./a", prefix);

        let prefix = get_file_path_prefix("./some/path/to/here", 19);
        assert_eq!("./some/path/to/here", prefix);

        let prefix = get_file_path_prefix("  ./a/b/c/d/e.cc", 12);
        assert_eq!("./a/b/c/d/", prefix);

        let prefix = get_file_path_prefix("no_prefix", 5);
        assert_eq!("", prefix);

        let prefix = get_file_path_prefix("/abs/path/to/file", 10);
        assert_eq!("/abs/path/", prefix);

        let prefix = get_file_path_prefix("../foo/bar", 3);
        assert_eq!("../", prefix);

        let prefix = get_file_path_prefix("../foo/bar", 7);
        assert_eq!("../foo/", prefix);
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
        assert!(items.iter().any(|s| s == "main.rs"));
        assert!(items.iter().any(|s| s == "trie.rs"));
        assert!(items.iter().any(|s| s == "file.rs"));
        assert!(items.iter().any(|s| s == "snippets.rs"));

        let items = list_all_file_items(Path::new("doesnt_exist"));
        assert_eq!(0, items.len());
    }
}
