use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

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

const MAX_LINE_LENGTH: usize = 600;

pub fn get_file_items(current_line: &str, root_folder: &str) -> Vec<(String, usize)> {
    if current_line.len() > MAX_LINE_LENGTH {
        return Vec::new();
    }
    
    let indices: Vec<usize> = current_line.char_indices().map(|(i, _)| i).collect();
    let mut file_items = Vec::new();
    for (j, _) in current_line.char_indices().filter(|&(_, ch)| ch == '/' || ch == '\\') {
        for &i in indices.iter() {
            if i > j {
                continue;
            }
            let p = &current_line[i..j+1];
            
            for base in [root_folder, ""].iter().map(PathBuf::from) {
                let path = base.join(p);
                file_items.extend(
                    list_all_file_items(&path)
                        .into_iter()
                        .map(|file_path| (file_path, j))
                );
            }
        }
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
    fn test_get_file_items() {
        // Create a dummy directory structure for testing
        fs::create_dir_all("./test_dir/subdir").unwrap();
        fs::File::create("./test_dir/file1.txt").unwrap();
        fs::File::create("./test_dir/subdir/file2.txt").unwrap();

        let line = "test_dir/subdir/";
        let root_folder = "./";
        let items = get_file_items(line, root_folder);

        assert!(items.contains(&("file2.txt".to_string(), 15)));

        // Clean up the dummy directory structure
        fs::remove_dir_all("./test_dir").unwrap();
    }
}
