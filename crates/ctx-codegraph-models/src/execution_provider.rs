//! ONNX Runtime execution provider selection with Apple Silicon CoreML support.

use ort::session::builder::SessionBuilder;
use ort::ep::CoreML;

use crate::error::ModelError;

/// ONNX execution provider preference for embedding inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmbeddingExecutionProvider {
    #[default]
    Auto,
    Cpu,
    CoreMl,
}

impl EmbeddingExecutionProvider {
    pub fn from_config_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "cpu" => Self::Cpu,
            "coreml" | "core_ml" | "apple" => Self::CoreMl,
            _ => Self::Auto,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Cpu => "cpu",
            Self::CoreMl => "coreml",
        }
    }
}

/// Apply execution providers to a session builder with platform-aware fallbacks.
pub fn configure_session_builder(
    builder: SessionBuilder,
    provider: EmbeddingExecutionProvider,
) -> Result<SessionBuilder, ModelError> {
    match provider {
        EmbeddingExecutionProvider::Cpu => Ok(builder),
        EmbeddingExecutionProvider::CoreMl => {
            #[cfg(target_os = "macos")]
            {
                return register_coreml(builder);
            }
            #[cfg(not(target_os = "macos"))]
            {
                eprintln!(
                    "Warning: CoreML requested on non-macOS platform; using CPU"
                );
                Ok(builder)
            }
        }
        EmbeddingExecutionProvider::Auto => {
            #[cfg(target_os = "macos")]
            {
                return register_coreml(builder);
            }
            #[cfg(not(target_os = "macos"))]
            {
                Ok(builder)
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn register_coreml(builder: SessionBuilder) -> Result<SessionBuilder, ModelError> {
    use ort::ep::coreml::{ComputeUnits, ModelFormat, SpecializationStrategy};

    let ep = CoreML::default()
        .with_model_format(ModelFormat::MLProgram)
        .with_specialization_strategy(SpecializationStrategy::FastPrediction)
        .with_compute_units(ComputeUnits::CPUAndNeuralEngine)
        .build();

    builder
        .with_execution_providers([ep])
        .map_err(|e| ModelError::Inference(format!("CoreML EP registration failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_from_config_str_parses_aliases() {
        assert_eq!(
            EmbeddingExecutionProvider::from_config_str("coreml"),
            EmbeddingExecutionProvider::CoreMl
        );
        assert_eq!(
            EmbeddingExecutionProvider::from_config_str("auto"),
            EmbeddingExecutionProvider::Auto
        );
        assert_eq!(
            EmbeddingExecutionProvider::from_config_str("cpu"),
            EmbeddingExecutionProvider::Cpu
        );
    }
}