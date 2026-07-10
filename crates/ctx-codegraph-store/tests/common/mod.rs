use ctx_codegraph_lang::backend::BackendRegistry;
use ctx_lang_python::PythonBackend;
use ctx_lang_rust::RustBackend;

pub fn production_registry() -> BackendRegistry {
    let mut reg = BackendRegistry::new();
    reg.register(Box::new(RustBackend::new()));
    reg.register(Box::new(PythonBackend::new()));
    reg
}