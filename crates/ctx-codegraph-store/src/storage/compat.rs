use ctx_codegraph_lang::backend::{BackendMetadata, BackendRegistry};
use ctx_codegraph_lang::CodeGraphError;
use ctx_codegraph_lang::index::BuildIndexOptions;
use ctx_codegraph_lang::model::{FileChangeDetection, RebuildReason};

pub fn check_db_compatibility_with_registry(
    conn: &rusqlite::Connection,
    options: &BuildIndexOptions,
    registry: &BackendRegistry,
) -> Result<Option<RebuildReason>, CodeGraphError> {
    // Check if metadata table exists
    let table_exists: bool = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='metadata'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .unwrap_or(false);

    if !table_exists {
        return Ok(Some(RebuildReason::MissingDatabase));
    }

    let get_meta = |key: &str| -> Option<String> {
        conn.query_row("SELECT value FROM metadata WHERE key = ?", [key], |row| {
            row.get::<_, String>(0)
        })
        .ok()
    };

    // 1. Schema version
    let schema_version = get_meta("schema_version");
    if schema_version.as_deref() != Some("5") {
        return Ok(Some(RebuildReason::SchemaVersionChanged));
    }

    let chunk_builder_version = get_meta("chunk_builder_version");
    if chunk_builder_version.as_deref() != Some("0.1.0") {
        return Ok(Some(RebuildReason::ChunkSchemaChanged));
    }

    // 2. Indexer version
    let indexer_version = get_meta("indexer_version");
    if indexer_version.as_deref() != Some("0.1.0") {
        return Ok(Some(RebuildReason::IndexerVersionChanged));
    }

    // 3. Parser version & config
    let expected_parser_config_hash = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(format!("include_tests:{}", options.include_tests).as_bytes());
        format!("{:x}", hasher.finalize())
    };
    let parser_config_hash = get_meta("parser_config_hash");
    if parser_config_hash.as_deref() != Some(&expected_parser_config_hash) {
        return Ok(Some(RebuildReason::ParserConfigChanged));
    }

    // 4. Resolver id, version & config
    let expected_resolver_id = if options.use_lsp { "lsp" } else { "noop" };
    let resolver_id = get_meta("resolver_id");
    if resolver_id.as_deref() != Some(expected_resolver_id) {
        return Ok(Some(RebuildReason::ResolverConfigChanged));
    }

    let resolver_version = get_meta("resolver_version");
    if resolver_version.as_deref() != Some("0.1.0") {
        return Ok(Some(RebuildReason::ResolverVersionChanged));
    }

    let expected_resolver_config_hash = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(
            format!(
                "use_lsp:{:?},max_depth:{:?}",
                options.use_lsp, options.max_depth
            )
            .as_bytes(),
        );
        format!("{:x}", hasher.finalize())
    };
    let resolver_config_hash = get_meta("resolver_config_hash");
    if resolver_config_hash.as_deref() != Some(&expected_resolver_config_hash) {
        return Ok(Some(RebuildReason::ResolverConfigChanged));
    }

    // 5. Change detection strategy
    let expected_change_detection = match options.change_detection {
        FileChangeDetection::MtimeAndSize => "MtimeAndSize",
        FileChangeDetection::ContentHash => "ContentHash",
    };
    let change_detection = get_meta("change_detection_strategy");
    if change_detection.as_deref() != Some(expected_change_detection) {
        return Ok(Some(RebuildReason::ChangeDetectionStrategyChanged));
    }

    // 6. Base index status
    let base_index_ready = get_meta("base_index_ready");
    if base_index_ready.as_deref() != Some("true") {
        return Ok(Some(RebuildReason::PreviousRunIncomplete));
    }

    // Check backends metadata
    let backends_metadata_str: Option<String> = conn
        .query_row(
            "SELECT value FROM metadata WHERE key = 'backends_metadata'",
            [],
            |row| row.get(0),
        )
        .ok();

    if let Some(meta_str) = backends_metadata_str {
        if let Ok(stored_metas) = serde_json::from_str::<Vec<BackendMetadata>>(&meta_str) {
            for stored in stored_metas {
                if let Some(backend) = registry
                    .all()
                    .iter()
                    .find(|b| b.id().0 == stored.backend_id)
                {
                    let current = backend.metadata(options);
                    if current.parser_version != stored.parser_version {
                        return Ok(Some(RebuildReason::ParserVersionChanged));
                    }
                    if current.resolver_id != stored.resolver_id
                        || current.config_hash != stored.config_hash
                    {
                        return Ok(Some(RebuildReason::ResolverConfigChanged));
                    }
                    if current.resolver_version != stored.resolver_version {
                        return Ok(Some(RebuildReason::ResolverVersionChanged));
                    }
                } else {
                    return Ok(Some(RebuildReason::BackendSetChanged));
                }
            }
        } else {
            return Ok(Some(RebuildReason::CorruptDatabase));
        }
    } else {
        return Ok(Some(RebuildReason::BackendSetChanged));
    }

    Ok(None)
}