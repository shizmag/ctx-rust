use ctx_codegraph_chunk::ChunkId;
use ctx_codegraph_lang::model::SymbolId;
use ctx_codegraph_search::{
    HybridQuery, HybridSearchBackend, HybridSearchOptions, HybridSearcher, SearchError,
    SearchResult,
};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
struct RecordedQuery {
    kind: &'static str,
    limit: usize,
}

enum BackendFailure {
    Lexical(SearchError),
}

struct RecordingBackend {
    lexical: Vec<SearchResult>,
    dense: Vec<SearchResult>,
    failure: Option<BackendFailure>,
    recorded: Arc<Mutex<Vec<RecordedQuery>>>,
}

impl RecordingBackend {
    fn new(
        lexical: Vec<SearchResult>,
        dense: Vec<SearchResult>,
    ) -> (Self, Arc<Mutex<Vec<RecordedQuery>>>) {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                lexical,
                dense,
                failure: None,
                recorded: Arc::clone(&recorded),
            },
            recorded,
        )
    }

    fn with_lexical_error(mut self, error: SearchError) -> Self {
        self.failure = Some(BackendFailure::Lexical(error));
        self
    }
}

impl HybridSearchBackend for RecordingBackend {
    fn search_lexical(&self, query: HybridQuery<'_>) -> Result<Vec<SearchResult>, SearchError> {
        self.recorded
            .lock()
            .unwrap()
            .push(RecordedQuery {
                kind: "lexical",
                limit: query.limit,
            });
        if let Some(BackendFailure::Lexical(err)) = &self.failure {
            return Err(match err {
                SearchError::IndexNotFound(msg) => SearchError::IndexNotFound(msg.clone()),
                SearchError::Other(msg) => SearchError::Other(msg.clone()),
            });
        }
        Ok(self.lexical.clone())
    }

    fn search_dense(&self, query: HybridQuery<'_>) -> Result<Vec<SearchResult>, SearchError> {
        self.recorded
            .lock()
            .unwrap()
            .push(RecordedQuery {
                kind: "dense",
                limit: query.limit,
            });
        Ok(self.dense.clone())
    }
}

struct FailingBackend {
    fail_lexical: AtomicUsize,
    fail_dense: AtomicUsize,
    lexical_results: Vec<SearchResult>,
    dense_results: Vec<SearchResult>,
}

impl FailingBackend {
    fn dense_error() -> Self {
        Self {
            fail_lexical: AtomicUsize::new(0),
            fail_dense: AtomicUsize::new(1),
            lexical_results: Vec::new(),
            dense_results: Vec::new(),
        }
    }
}

impl HybridSearchBackend for FailingBackend {
    fn search_lexical(&self, _query: HybridQuery<'_>) -> Result<Vec<SearchResult>, SearchError> {
        if self.fail_lexical.fetch_sub(1, Ordering::SeqCst) > 0 {
            Err(SearchError::Other("lexical failed".to_string()))
        } else {
            Ok(self.lexical_results.clone())
        }
    }

    fn search_dense(&self, _query: HybridQuery<'_>) -> Result<Vec<SearchResult>, SearchError> {
        if self.fail_dense.fetch_sub(1, Ordering::SeqCst) > 0 {
            Err(SearchError::Other("dense failed".to_string()))
        } else {
            Ok(self.dense_results.clone())
        }
    }
}

fn sample_query(limit: usize) -> HybridQuery<'static> {
    HybridQuery {
        workspace_root: Path::new("/tmp/workspace"),
        text: "find me",
        limit,
    }
}

#[test]
fn hybrid_search_options_default() {
    let options = HybridSearchOptions::default();
    assert_eq!(options.rrf_k, 60);
    assert_eq!(options.lexical_top_k, 50);
    assert_eq!(options.dense_top_k, 50);
}

#[test]
fn hybrid_search_fuses_lexical_and_dense_results() {
    let lexical = vec![SearchResult {
        chunk_id: ChunkId(1),
        symbol_id: SymbolId(10),
        score: 0.9,
        snippet: None,
    }];
    let dense = vec![SearchResult {
        chunk_id: ChunkId(2),
        symbol_id: SymbolId(20),
        score: 0.95,
        snippet: None,
    }];
    let (backend, _) = RecordingBackend::new(lexical, dense);
    let searcher = HybridSearcher::new(
        backend,
        HybridSearchOptions {
            rrf_k: 60,
            lexical_top_k: 10,
            dense_top_k: 10,
        },
    );

    let results = searcher.search(sample_query(5)).expect("hybrid search");

    assert_eq!(results.len(), 2);
    assert!(results.iter().any(|r| r.chunk_id == ChunkId(1)));
    assert!(results.iter().any(|r| r.chunk_id == ChunkId(2)));
    for result in &results {
        assert_eq!(result.snippet, None);
        assert!(result.score > 0.0);
    }
}

#[test]
fn hybrid_search_passes_backend_top_k_limits() {
    let (backend, recorded) = RecordingBackend::new(Vec::new(), Vec::new());
    let searcher = HybridSearcher::new(
        backend,
        HybridSearchOptions {
            rrf_k: 60,
            lexical_top_k: 17,
            dense_top_k: 23,
        },
    );

    searcher.search(sample_query(5)).expect("hybrid search");

    let queries = recorded.lock().unwrap();
    assert_eq!(queries.len(), 2);
    assert!(queries.iter().any(|q| q.kind == "lexical" && q.limit == 17));
    assert!(queries.iter().any(|q| q.kind == "dense" && q.limit == 23));
}

#[test]
fn hybrid_search_respects_result_limit() {
    let lexical = vec![
        SearchResult {
            chunk_id: ChunkId(1),
            symbol_id: SymbolId(1),
            score: 0.9,
            snippet: None,
        },
        SearchResult {
            chunk_id: ChunkId(2),
            symbol_id: SymbolId(2),
            score: 0.8,
            snippet: None,
        },
        SearchResult {
            chunk_id: ChunkId(3),
            symbol_id: SymbolId(3),
            score: 0.7,
            snippet: None,
        },
    ];
    let (backend, _) = RecordingBackend::new(lexical, Vec::new());
    let searcher = HybridSearcher::new(backend, HybridSearchOptions::default());

    let results = searcher.search(sample_query(2)).expect("hybrid search");
    assert_eq!(results.len(), 2);
}

#[test]
fn hybrid_search_limit_zero_becomes_one() {
    let lexical = vec![
        SearchResult {
            chunk_id: ChunkId(1),
            symbol_id: SymbolId(1),
            score: 0.9,
            snippet: None,
        },
        SearchResult {
            chunk_id: ChunkId(2),
            symbol_id: SymbolId(2),
            score: 0.8,
            snippet: None,
        },
    ];
    let (backend, _) = RecordingBackend::new(lexical, Vec::new());
    let searcher = HybridSearcher::new(backend, HybridSearchOptions::default());

    let results = searcher.search(sample_query(0)).expect("hybrid search");
    assert_eq!(results.len(), 1);
}

#[test]
fn hybrid_search_preserves_symbol_ids_from_backends() {
    let lexical = vec![SearchResult {
        chunk_id: ChunkId(42),
        symbol_id: SymbolId(99),
        score: 0.0,
        snippet: None,
    }];
    let (backend, _) = RecordingBackend::new(lexical, Vec::new());
    let searcher = HybridSearcher::new(backend, HybridSearchOptions::default());

    let results = searcher.search(sample_query(1)).expect("hybrid search");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].symbol_id, SymbolId(99));
}

#[test]
fn hybrid_search_propagates_lexical_error() {
    let (backend, _) = RecordingBackend::new(Vec::new(), Vec::new());
    let backend = backend.with_lexical_error(SearchError::IndexNotFound("lexical".to_string()));
    let searcher = HybridSearcher::new(backend, HybridSearchOptions::default());

    let err = searcher.search(sample_query(5)).unwrap_err();
    assert!(matches!(err, SearchError::IndexNotFound(_)));
}

#[test]
fn hybrid_search_propagates_dense_error() {
    let backend = FailingBackend::dense_error();
    let searcher = HybridSearcher::new(backend, HybridSearchOptions::default());

    let err = searcher.search(sample_query(5)).unwrap_err();
    assert!(matches!(err, SearchError::Other(_)));
}