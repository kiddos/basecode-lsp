mod basecode_lsp;

use std::env;

use basecode_lsp::backend::*;
use clap::Parser;
use simple_log::error;
use simple_log::LogConfigBuilder;
use tower_lsp::{LspService, Server};

fn setup_debug_logging() {
    let mut temp_dir = env::temp_dir();
    temp_dir.push("baselsp.log");
    if let Some(log_path) = temp_dir.to_str() {
        let config = LogConfigBuilder::builder().path(log_path).build();
        if let Err(_e) = simple_log::new(config) {
            error!("fail to setup log {}", log_path);
            return;
        }
    }
}

#[tokio::main]
async fn main() {
    let args = LspArgs::parse();

    if args.debug {
        setup_debug_logging();
    }

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let backend = Backend::new(args);
    let (service, socket) = LspService::new(|_| backend);
    Server::new(stdin, stdout, socket).serve(service).await;
}
