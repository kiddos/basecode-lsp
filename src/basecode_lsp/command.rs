use std::env;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    if let Ok(metadata) = fs::metadata(path) {
        let permissions = metadata.permissions();
        return metadata.is_file() && (permissions.mode() & 0o111 != 0);
    }
    false
}

#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    if let Ok(metadata) = fs::metadata(path) {
        if !metadata.is_file() {
            return false;
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_upper = format!(".{}", ext.to_uppercase());
            let pathext = env::var("PATHEXT")
                .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC".to_string());
            return pathext.split(';').any(|e| e.eq_ignore_ascii_case(&ext_upper));
        }
    }
    false
}

#[cfg(not(any(unix, windows)))]
fn is_executable(path: &Path) -> bool {
    fs::metadata(path).map(|m| m.is_file()).unwrap_or(false)
}

pub fn get_command_completions() -> Vec<String> {
    let mut commands = Vec::new();
    if let Ok(path_var) = env::var("PATH") {
        for path in env::split_paths(&path_var) {
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
        #[cfg(unix)]
        {
            assert!(items.iter().any(|s| s == "cp"));
            assert!(items.iter().any(|s| s == "mv"));
            assert!(items.iter().any(|s| s == "ls"));
        }
        assert_ne!(0, items.len());
    }
}
