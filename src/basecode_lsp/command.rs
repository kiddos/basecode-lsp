use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn is_executable(path: &Path) -> bool {
    if let Ok(metadata) = fs::metadata(path) {
        let permissions = metadata.permissions();
        return metadata.is_file() && (permissions.mode() & 0o111 != 0);
    }
    false
}

pub fn get_command_completions() -> Vec<String> {
    let mut commands = Vec::new();
    if let Ok(path_var) = env::var("PATH") {
        for path in path_var.split(':') {
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if is_executable(&path) {
                        if let Some(command) = path.file_name() {
                            if let Some(command_str) = command.to_str() {
                                commands.push(command_str.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_command_completion() {
        let items = get_command_completions();
        // for item in items.iter() {
        //     println!("item = {}", item);
        // }
        assert!(items.iter().any(|s| s == "cp"));
        assert!(items.iter().any(|s| s == "mv"));
        assert!(items.iter().any(|s| s == "ls"));
        assert_ne!(0, items.len());
    }
}
