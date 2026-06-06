# basecode-lsp

`basecode-lsp` is a versatile, high-performance Language Server Protocol (LSP) implementation designed to provide intelligent snippet completion, context-aware text suggestions, and keyword recognition across multiple programming languages.

Built with Rust, it aims to enhance the development experience by offering seamless integration with various editors and IDEs, providing fast and relevant completions from various sources.

## ✨ Features

- 🧠 **Smart Word Completion**: Indexes words from all open documents using a high-performance Trie for instant prefix-based suggestions.
- 📂 **Snippet Support**: Extensible snippet system supporting custom `.snippets` files with filetype-specific recognition.
- 🛣️ **File Path Completion**: Context-aware file path suggestions relative to your project root.
- 🐚 **Tmux Integration**: Pulls context from active `tmux` panes to provide completions from your entire terminal environment.
- ⚡ **Performance Optimized**:
    - **Debounced Indexing**: Intelligent 500ms debounce on document changes to minimize CPU overhead.
    - **Trie-based Search**: Extremely fast word suggestions even in large codebases.
    - **Full Async**: Built on `tokio` and `tower-lsp` for non-blocking performance.

## 🚀 Installation

To install `basecode-lsp`, ensure you have [Rust](https://www.rust-lang.org/tools/install) installed, then run:

```bash
cargo install --git https://github.com/kiddos/basecode-lsp.git
```

## 🛠️ Configuration

`basecode-lsp` can be configured via command-line arguments when starting the server:

| Argument | Description | Default |
| :--- | :--- | :--- |
| `--snippet-folder` | Path to the directory containing `.snippets` files | None |
| `--root-folder`    | Root path for relative file path completion | None |
| `--min-word-len`   | Minimum length for words to be indexed | `2` |
| `--tmux-source`    | Enable/disable pulling words from tmux panes | `true` |
| `--command-source` | Enable/disable command completion from PATH | `false` |
| `--debug`          | Enable debug logging to a temporary file | `false` |

### Snippet Format

Create `.snippets` files (e.g., `rust.snippets`) in your snippet folder:

```snippets
snippet main
	fn main() {
		${0}
	}
```

## 🔌 Editor Integration

### Neovim

Using `nvim-lspconfig`:

```lua
local configs = require('lspconfig.configs')
if not configs.basecode_lsp then
  configs.basecode_lsp = {
    default_config = {
      cmd = { 'basecode-lsp', '--snippet-folder', '/path/to/snippets' },
      filetypes = { 'rust', 'c', 'cpp', 'javascript', 'python', 'sh' },
      root_dir = require('lspconfig.util').root_pattern('.git', 'Cargo.toml'),
      settings = {},
    },
  }
end
require('lspconfig').basecode_lsp.setup({})
```

## 🧪 Technical Overview

- **Storage**: Uses a Trie structure for efficient word storage and prefix matching.
- **Indexing**: Words are tokenized on whitespace and alphanumeric sequences.
- **Async Architecture**: Leverages `tokio` for handling multiple concurrent requests and background tasks (like tmux updates).
- **Tmux Context**: Captures content from all panes in the current tmux session using `tmux capture-pane`.

## 📄 License

Distributed under the MIT License.
