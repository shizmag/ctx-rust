pub mod lsp_definition;
pub mod lsp_transport;

#[cfg(test)]
mod resolver_tests;

pub use lsp_definition::{LocationParser, LspDefinitionResolver, LspServerConfig};
pub use lsp_transport::GenericLspClient;
pub use ctx_codegraph_lang::noop::{parse_raw_name, resolve_name_only, resolve_name_only_occurrence};