use regex::Regex;
use std::process::Command;
use super::util::*;

fn is_tmux_executable() -> bool {
    let output = Command::new("tmux").arg("-V").output();
    match output {
        Ok(_) => true,
        Err(_) => false,
    }
}

fn list_tmux_panes() -> Vec<String> {
    // Execute the tmux list-panes command to get all panes
    let panes_output = Command::new("tmux")
        .arg("list-panes")
        .arg("-a")
        .arg("-F")
        .arg("#{pane_id}")
        .output();
    if let Ok(output) = panes_output {
        if !output.stdout.is_empty() {
            let pane_ids: Vec<String> = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|line| line.to_string())
                .collect();
            return pane_ids;
        }
    }
    Vec::new()
}

fn capture_tmux_pane(pane_id: &str) -> Result<String, ()> {
    let command_output = Command::new("tmux")
        .arg("capture-pane")
        .arg("-p")
        .arg("-t")
        .arg(pane_id)
        .output();

    if let Ok(output) = command_output {
        if !output.stdout.is_empty() {
            return Ok(String::from_utf8_lossy(&output.stdout).to_string());
        }
    }
    Err(())
}

fn capture_alphanumeric_sequences(input: &str) -> Vec<String> {
    let re = Regex::new(r"[a-zA-Z0-9]+").unwrap();

    re.find_iter(input)
        .map(|mat| mat.as_str().to_string())
        .collect()
}

pub fn retrieve_tmux_words(min_len: usize) -> Vec<String> {
    if !is_tmux_executable() {
        return Vec::new();
    }
    let panes = list_tmux_panes();
    let mut result: Vec<String> = Vec::new();
    for pane in panes.iter() {
        if let Ok(content) = capture_tmux_pane(pane) {
            let words = capture_alphanumeric_sequences(&content);
            result.extend(words);
        }
    }

    result = result
        .into_iter()
        .filter(|s| is_token(&s.chars().collect(), min_len))
        .collect();
    result.sort();
    result.dedup();
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_is_tmux_executable() {
        println!("{:?}", super::is_tmux_executable());
    }

    #[test]
    fn test_list_tmux_panes() {
        if super::is_tmux_executable() {
            let panes = super::list_tmux_panes();
            println!("{:?}", panes);
        }
    }

    #[test]
    fn test_capture_alphanumeric_sequences() {
        let input = "Hello, world! 123 abc_def";
        let words = super::capture_alphanumeric_sequences(input);
        assert_eq!(words, vec!["Hello", "world", "123", "abc", "def"]);
    }

    #[test]
    fn test_capture_tmux_pane() {
        if super::is_tmux_executable() {
            let panes = super::list_tmux_panes();
            if panes.is_empty() {
                return;
            }
            let pane_id = &panes[0];
            let content = super::capture_tmux_pane(pane_id);
            assert!(content.is_ok());
        }
    }

    #[test]
    fn test_retrieve_tmux_words() {
        if super::is_tmux_executable() {
            let words = super::retrieve_tmux_words(3);
            println!("{:?}", words);
        }
    }
}
