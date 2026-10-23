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

    #[test]
    fn test_is_executable() {
        let dir = env::temp_dir();

        // Use a unique file name instead of a static one to avoid race conditions.
        let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let exec_file_path = dir.join(format!("test_is_executable_exec_{}.sh", timestamp));
        fs::write(&exec_file_path, "#!/bin/bash\necho hello").unwrap();

        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&exec_file_path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&exec_file_path, perms).unwrap();

            assert!(is_executable(&exec_file_path));
        }

        let non_exec_file_path = dir.join(format!("test_is_executable_nonexec_{}.txt", timestamp));
        fs::write(&non_exec_file_path, "hello world").unwrap();

        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&non_exec_file_path).unwrap().permissions();
            perms.set_mode(0o644);
            fs::set_permissions(&non_exec_file_path, perms).unwrap();

            assert!(!is_executable(&non_exec_file_path));
        }

        assert!(!is_executable(&dir)); // Directories should return false

        let _ = fs::remove_file(&exec_file_path);
        let _ = fs::remove_file(&non_exec_file_path);
    }
}
