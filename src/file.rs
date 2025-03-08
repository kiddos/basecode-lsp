use regex::Regex;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

pub fn find_file_paths(input: &str) -> Vec<String> {
    let pattern = r#"(\./[^\s<>:\",|?*]+(?:/[^\s<>:\",|?*]+)*|\.\./[^\s<>:\"|?*]+(?:/[^\s<>:\"|?*]+)*|[a-zA-Z]:\\[^\s<>:\",|?*]+(?:\\[^\s<>:\",|?*]+)*|/[^<>\s:\",|?*\r\n]+(?:/[^<>:\"|?*\r\n]+)*)"#;
    let re = Regex::new(pattern).unwrap();
    re.find_iter(input)
        .map(|m| m.as_str().to_string())
        .collect()
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

pub fn get_file_items(text: &str, root_folder: &str) -> Vec<String> {
    let mut file_items = Vec::new();
    let file_paths = find_file_paths(&text);
    // info!("file path size: {}", file_paths.len());
    for file_path in file_paths.iter() {
        let mut root = PathBuf::from(root_folder);
        root = root.join(file_path);
        if !root.is_dir() {
            root = root.parent().map(|p| p.to_path_buf()).unwrap();
        }
        let possible = list_all_file_items(&root);
        file_items.extend(possible);
        // info!("file path: {}", file_path);
    }
    file_items.sort();
    file_items.dedup();
    file_items
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_find_file_paths() {
        let example = "path/to/my_file.txt";
        assert_eq!(find_file_paths(example), vec!["/to/my_file.txt"]);

        let example = "Check these paths: C:\\Users\\User\\file.txt, /home/user/docs/report.pdf";
        assert_eq!(
            find_file_paths(example),
            vec!["C:\\Users\\User\\file.txt", "/home/user/docs/report.pdf"]
        );

        let example = "./file.txt";
        assert_eq!(find_file_paths(example), vec!["./file.txt"]);

        let example = "./";
        assert_eq!(find_file_paths(example), Vec::<String>::new());

        let example = "Some text with a path ./src/main.rs in it.";
        assert_eq!(find_file_paths(example), vec!["./src/main.rs"]);

        let example = "Multiple paths: ./file1.txt, ./file2.txt, ./file3.txt";
        assert_eq!(
            find_file_paths(example),
            vec!["./file1.txt", "./file2.txt", "./file3.txt"]
        );

        let example = "A path with spaces: /path/to/a_long_name_file.txt";
        assert_eq!(find_file_paths(example), vec!["/path/to/a_long_name_file.txt"]);

        let example = "A relative path: ../parent/file.txt";
        assert_eq!(find_file_paths(example), vec!["../parent/file.txt"]);

        let example = "Just the drive letter: C:\\";
        assert_eq!(find_file_paths(example), Vec::<String>::new());

        let example = "A more complex windows path: D:\\MyDocuments\\Project\\file.pdf";
        assert_eq!(
            find_file_paths(example),
            vec!["D:\\MyDocuments\\Project\\file.pdf"]
        );

        let example =
            "Mix of paths: ./local/file.txt and /absolute/file.txt and C:\\windows\\file.exe";
        assert_eq!(
            find_file_paths(example),
            vec![
                "./local/file.txt",
                "/absolute/file.txt",
                "C:\\windows\\file.exe"
            ]
        );

        let example = "No paths here.";
        assert_eq!(find_file_paths(example), Vec::<String>::new());

        let example = "Path at the end: /opt/app/data";
        assert_eq!(find_file_paths(example), vec!["/opt/app/data"]);

        let example =
            "This is a test with a long path: /very/long/path/to/a/deeply/nested/file.txt";
        assert_eq!(
            find_file_paths(example),
            vec!["/very/long/path/to/a/deeply/nested/file.txt"]
        );

        let example = "This is a test with a long windows path: C:\\very\\long\\path\\to\\a\\deeply\\nested\\file.txt";
        assert_eq!(
            find_file_paths(example),
            vec!["C:\\very\\long\\path\\to\\a\\deeply\\nested\\file.txt"]
        );

        let example = "This is a test with a long relative path: ./very/long/path/to/a/deeply/nested/file.txt";
        assert_eq!(
            find_file_paths(example),
            vec!["./very/long/path/to/a/deeply/nested/file.txt"]
        );
    }
}
