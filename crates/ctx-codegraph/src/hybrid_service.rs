use crate::context::{HybridRetrievalOptions, RetrievalStrategy};
use crate::error::CodeGraphError;
use crate::service::GraphContextService;
use crate::WorkspaceHybridBackend;
use crate::{ContextBudget, ContextPack, retrieve_context_with_options};
use ctx_config::Config;

pub fn retrieve_context_for_service(
    service: &GraphContextService,
    query: &str,
    budget: &ContextBudget,
    options: &HybridRetrievalOptions,
    config: &Config,
) -> Result<ContextPack, CodeGraphError> {
    if options.strategy == RetrievalStrategy::Graph {
        let conn = service.lock_conn();
        return crate::retrieve_graph_context_with_options(
            &conn,
            query,
            budget,
            &options.graph_options,
        );
    }

    let backend = WorkspaceHybridBackend::try_with_config(service.repo_root(), config)
        .map_err(|e| CodeGraphError::Parse(e.to_string()))?
        .ok_or_else(|| {
            CodeGraphError::Parse(
                "hybrid search not configured; set embedding_model in .ctxconfig and rebuild_index"
                    .into(),
            )
        })?;
    let conn = service.lock_conn();
    retrieve_context_with_options(
        &conn,
        service.repo_root(),
        &backend,
        query,
        budget,
        options,
    )
}