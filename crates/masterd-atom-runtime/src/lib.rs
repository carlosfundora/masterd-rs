use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use rs_rotorquant_codec::{RotorQuantCodec, RotorQuantMode};
pub use rs_turboquant_codec::{BitWidth, CodecInfo, TurboQuantizer};

pub const GGUF_HEADER_BYTES: usize = 24;
const GGUF_MAGIC: [u8; 4] = *b"GGUF";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GgufHeaderV3 {
    pub version: u32,
    pub tensor_count: u64,
    pub metadata_kv_count: u64,
}

impl GgufHeaderV3 {
    pub fn estimated_index_bytes(&self) -> u64 {
        (self.tensor_count.saturating_mul(64)) + (self.metadata_kv_count.saturating_mul(32))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GgufLoadPlan {
    pub prefetch_bytes: u64,
    pub io_chunk_bytes: u64,
    pub use_mmap: bool,
    pub use_pinned_staging: bool,
}

#[derive(Debug, Error)]
pub enum GgufError {
    #[error("failed to read gguf file: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid gguf magic: {0:?}")]
    InvalidMagic([u8; 4]),
    #[error("unsupported gguf version: {0}")]
    UnsupportedVersion(u32),
    #[error("truncated gguf header: expected at least {expected} bytes, got {actual}")]
    TruncatedHeader { expected: usize, actual: usize },
}

pub fn parse_gguf_header_bytes(raw: &[u8]) -> Result<GgufHeaderV3, GgufError> {
    if raw.len() < GGUF_HEADER_BYTES {
        return Err(GgufError::TruncatedHeader {
            expected: GGUF_HEADER_BYTES,
            actual: raw.len(),
        });
    }

    let magic = [raw[0], raw[1], raw[2], raw[3]];
    if magic != GGUF_MAGIC {
        return Err(GgufError::InvalidMagic(magic));
    }

    let version = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]);
    if version < 3 {
        return Err(GgufError::UnsupportedVersion(version));
    }

    let tensor_count = u64::from_le_bytes([
        raw[8], raw[9], raw[10], raw[11], raw[12], raw[13], raw[14], raw[15],
    ]);
    let metadata_kv_count = u64::from_le_bytes([
        raw[16], raw[17], raw[18], raw[19], raw[20], raw[21], raw[22], raw[23],
    ]);

    Ok(GgufHeaderV3 {
        version,
        tensor_count,
        metadata_kv_count,
    })
}

pub fn parse_gguf_header_path(path: &Path) -> Result<GgufHeaderV3, GgufError> {
    let mut file = std::fs::File::open(path)?;
    let mut header = [0u8; GGUF_HEADER_BYTES];
    file.read_exact(&mut header)?;
    parse_gguf_header_bytes(&header)
}

pub fn synthesize_load_plan(header: &GgufHeaderV3, max_prefetch_mb: u64) -> GgufLoadPlan {
    let max_prefetch_bytes = max_prefetch_mb.saturating_mul(1024 * 1024);
    let prefetch_bytes = header.estimated_index_bytes().min(max_prefetch_bytes);
    let io_chunk_bytes = if prefetch_bytes <= 4 * 1024 * 1024 {
        1024 * 1024
    } else if prefetch_bytes <= 64 * 1024 * 1024 {
        4 * 1024 * 1024
    } else {
        8 * 1024 * 1024
    };

    GgufLoadPlan {
        prefetch_bytes,
        io_chunk_bytes,
        use_mmap: prefetch_bytes >= 8 * 1024 * 1024,
        use_pinned_staging: prefetch_bytes >= 2 * 1024 * 1024,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRuntimeRole {
    Chat,
    DenseEmbedding,
    LateInteractionRerank,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAssetFormat {
    Gguf,
    Safetensors,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreferredRuntime {
    RustCandleGguf,
    PythonTransformersSidecar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MasterdModelRuntimeSpec {
    pub id: &'static str,
    pub role: ModelRuntimeRole,
    pub asset_format: ModelAssetFormat,
    pub preferred_runtime: PreferredRuntime,
    pub model_dir: &'static str,
    pub weights: &'static str,
    pub tokenizer: &'static str,
    pub min_weight_bytes: u64,
}

pub const MASTERD_MODEL_RUNTIME_SPECS: &[MasterdModelRuntimeSpec] = &[
    MasterdModelRuntimeSpec {
        id: "lfm2.5-thinking-1.2b",
        role: ModelRuntimeRole::Chat,
        asset_format: ModelAssetFormat::Gguf,
        preferred_runtime: PreferredRuntime::RustCandleGguf,
        model_dir: "models/lfm2.5-1.2b-thinking",
        weights: "LFM2.5-1.2B-Thinking-Q8_0.gguf",
        tokenizer: "tokenizer.json",
        min_weight_bytes: 1_000_000_000,
    },
    MasterdModelRuntimeSpec {
        id: "lfm2.5-instruct-350m",
        role: ModelRuntimeRole::Chat,
        asset_format: ModelAssetFormat::Gguf,
        preferred_runtime: PreferredRuntime::RustCandleGguf,
        model_dir: "models/lfm2.5-350m-instruct",
        weights: "LFM2.5-350M-Q8_0.gguf",
        tokenizer: "tokenizer.json",
        min_weight_bytes: 300_000_000,
    },
    MasterdModelRuntimeSpec {
        id: "lfm2-colbert-350m",
        role: ModelRuntimeRole::LateInteractionRerank,
        asset_format: ModelAssetFormat::Gguf,
        preferred_runtime: PreferredRuntime::RustCandleGguf,
        model_dir: "models/lfm2-colbert-350m",
        weights: "LFM2-ColBERT-350M-Q8_0.gguf",
        tokenizer: "tokenizer.json",
        min_weight_bytes: 300_000_000,
    },
    MasterdModelRuntimeSpec {
        id: "jina-v5-omni-nano",
        role: ModelRuntimeRole::DenseEmbedding,
        asset_format: ModelAssetFormat::Safetensors,
        preferred_runtime: PreferredRuntime::PythonTransformersSidecar,
        model_dir: "models/jina-v5-omni-nano",
        weights: "model.safetensors",
        tokenizer: "tokenizer.json",
        min_weight_bytes: 1_500_000_000,
    },
    MasterdModelRuntimeSpec {
        id: "jina-v5-omni-small",
        role: ModelRuntimeRole::DenseEmbedding,
        asset_format: ModelAssetFormat::Safetensors,
        preferred_runtime: PreferredRuntime::PythonTransformersSidecar,
        model_dir: "models/jina-v5-omni-small",
        weights: "model.safetensors",
        tokenizer: "tokenizer.json",
        min_weight_bytes: 2_500_000_000,
    },
];

pub fn runtime_spec(id: &str) -> Option<&'static MasterdModelRuntimeSpec> {
    MASTERD_MODEL_RUNTIME_SPECS
        .iter()
        .find(|spec| spec.id == id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionBackend {
    CandleCpu,
    CandleMetal,
    CandleRocm,
    AiterTriton,
    Aotriton,
    DeepSpeedHip,
    FlashAttention3Bridge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AmdGpuClass {
    GenericRocm,
    Rdna2,
    Rdna3,
    Cdna2,
    Cdna3,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelCapability {
    pub id: &'static str,
    pub backend: AttentionBackend,
    pub runtime_available: bool,
    pub cpu_safe: bool,
    pub amd_safe: bool,
    pub amd_gpu_class: Option<AmdGpuClass>,
    pub notes: &'static str,
}

pub const MASTERD_KERNEL_CAPABILITIES: &[KernelCapability] = &[
    KernelCapability {
        id: "candle_cpu_lfm2_gguf",
        backend: AttentionBackend::CandleCpu,
        runtime_available: true,
        cpu_safe: true,
        amd_safe: true,
        amd_gpu_class: None,
        notes: "Current in-process Rust path for LFM GGUF chat models.",
    },
    KernelCapability {
        id: "turboquant_kv_codec",
        backend: AttentionBackend::FlashAttention3Bridge,
        runtime_available: true,
        cpu_safe: true,
        amd_safe: true,
        amd_gpu_class: None,
        notes: "Pure Rust TurboQuant codec imported from gfxATOM; usable for KV payload compression and tests.",
    },
    KernelCapability {
        id: "rotorquant_kv_codec",
        backend: AttentionBackend::FlashAttention3Bridge,
        runtime_available: true,
        cpu_safe: true,
        amd_safe: true,
        amd_gpu_class: None,
        notes: "Pure Rust RotorQuant codec imported from gfxATOM; usable for KV payload compression and tests.",
    },
    KernelCapability {
        id: "generic_rocm_attention_candidate",
        backend: AttentionBackend::CandleRocm,
        runtime_available: false,
        cpu_safe: false,
        amd_safe: true,
        amd_gpu_class: Some(AmdGpuClass::GenericRocm),
        notes: "Capability placeholder for non-RDNA2 AMD systems; enabled only after a linked ROCm backend is present.",
    },
    KernelCapability {
        id: "fa3_descale_bridge",
        backend: AttentionBackend::FlashAttention3Bridge,
        runtime_available: false,
        cpu_safe: false,
        amd_safe: true,
        amd_gpu_class: Some(AmdGpuClass::Cdna3),
        notes: "ATOM-RS has metadata/bridge scaffolding, but MASTERd does not link a real ROCm FA3 kernel yet.",
    },
    KernelCapability {
        id: "aiter_triton_attention",
        backend: AttentionBackend::AiterTriton,
        runtime_available: false,
        cpu_safe: false,
        amd_safe: true,
        amd_gpu_class: Some(AmdGpuClass::GenericRocm),
        notes: "Available as an external ATOM/SGLang path; not copied into MASTERd until native build checks pass.",
    },
    KernelCapability {
        id: "aotriton_attention",
        backend: AttentionBackend::Aotriton,
        runtime_available: false,
        cpu_safe: false,
        amd_safe: true,
        amd_gpu_class: Some(AmdGpuClass::GenericRocm),
        notes: "Available as an external ATOM path; not linked into MASTERd yet.",
    },
    KernelCapability {
        id: "rdna2_wave32_dispatch_candidate",
        backend: AttentionBackend::DeepSpeedHip,
        runtime_available: false,
        cpu_safe: false,
        amd_safe: true,
        amd_gpu_class: Some(AmdGpuClass::Rdna2),
        notes: "Known ATOM dispatch family for gfx1030/RDNA2; kept separate from generic ROCm.",
    },
    KernelCapability {
        id: "rdna3_wave32_dispatch_candidate",
        backend: AttentionBackend::DeepSpeedHip,
        runtime_available: false,
        cpu_safe: false,
        amd_safe: true,
        amd_gpu_class: Some(AmdGpuClass::Rdna3),
        notes: "Non-RDNA2 AMD candidate path for newer consumer GPUs; not claimed linked yet.",
    },
    KernelCapability {
        id: "cdna_dispatch_candidate",
        backend: AttentionBackend::AiterTriton,
        runtime_available: false,
        cpu_safe: false,
        amd_safe: true,
        amd_gpu_class: Some(AmdGpuClass::Cdna2),
        notes: "Datacenter AMD candidate path; not claimed linked yet.",
    },
];

pub fn runtime_kernel_capabilities() -> &'static [KernelCapability] {
    MASTERD_KERNEL_CAPABILITIES
}

pub fn cpu_acceleration_flags() -> Vec<&'static str> {
    let mut flags = vec!["scalar"];
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::arch::is_x86_feature_detected!("sse2") {
            flags.push("sse2");
        }
        if std::arch::is_x86_feature_detected!("avx2") {
            flags.push("avx2");
        }
        if std::arch::is_x86_feature_detected!("fma") {
            flags.push("fma");
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        flags.push("neon");
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::{
        BitWidth, GGUF_HEADER_BYTES, GgufError, GgufHeaderV3, MASTERD_KERNEL_CAPABILITIES,
        MASTERD_MODEL_RUNTIME_SPECS, ModelAssetFormat, PreferredRuntime, RotorQuantCodec,
        RotorQuantMode, TurboQuantizer, parse_gguf_header_bytes, synthesize_load_plan,
    };

    fn sample_header_bytes(version: u32, tensors: u64, kv: u64) -> [u8; GGUF_HEADER_BYTES] {
        let mut out = [0u8; GGUF_HEADER_BYTES];
        out[0..4].copy_from_slice(b"GGUF");
        out[4..8].copy_from_slice(&version.to_le_bytes());
        out[8..16].copy_from_slice(&tensors.to_le_bytes());
        out[16..24].copy_from_slice(&kv.to_le_bytes());
        out
    }

    #[test]
    fn parses_v3_header() {
        let raw = sample_header_bytes(3, 4096, 128);
        let header = parse_gguf_header_bytes(&raw).expect("header should parse");
        assert_eq!(
            header,
            GgufHeaderV3 {
                version: 3,
                tensor_count: 4096,
                metadata_kv_count: 128
            }
        );
    }

    #[test]
    fn rejects_invalid_magic() {
        let mut raw = sample_header_bytes(3, 1, 1);
        raw[0] = b'X';
        assert!(matches!(
            parse_gguf_header_bytes(&raw),
            Err(GgufError::InvalidMagic(_))
        ));
    }

    #[test]
    fn load_plan_scales_prefetch_and_flags() {
        let header = GgufHeaderV3 {
            version: 3,
            tensor_count: 500_000,
            metadata_kv_count: 2_000,
        };
        let plan = synthesize_load_plan(&header, 64);
        assert_eq!(plan.prefetch_bytes, header.estimated_index_bytes());
        assert!(plan.use_mmap);
        assert!(plan.use_pinned_staging);
    }

    #[test]
    fn runtime_specs_keep_jina_python_until_native_runner_exists() {
        let jina = MASTERD_MODEL_RUNTIME_SPECS
            .iter()
            .find(|spec| spec.id == "jina-v5-omni-nano")
            .expect("jina spec");
        assert_eq!(jina.asset_format, ModelAssetFormat::Safetensors);
        assert_eq!(
            jina.preferred_runtime,
            PreferredRuntime::PythonTransformersSidecar
        );
    }

    #[test]
    fn imported_quant_codecs_are_constructible() {
        let turbo = TurboQuantizer::new(64, BitWidth::Bit2 as u8, 16, 42);
        assert!(turbo.is_ok());

        let rotor = RotorQuantCodec::new(RotorQuantMode::PlanarQuant3, 42, true);
        let encoded = rotor.compress_planar(&[0.1, -0.2, 0.3, -0.4], 4);
        assert!(!encoded.is_empty());
    }

    #[test]
    fn kernel_capabilities_do_not_claim_unlinked_rocm_kernels() {
        let linked = MASTERD_KERNEL_CAPABILITIES
            .iter()
            .filter(|capability| capability.runtime_available)
            .map(|capability| capability.id)
            .collect::<Vec<_>>();
        assert!(linked.contains(&"turboquant_kv_codec"));
        assert!(linked.contains(&"rotorquant_kv_codec"));
        assert!(linked.contains(&"candle_cpu_lfm2_gguf"));
        assert!(!linked.contains(&"aiter_triton_attention"));
        assert!(!linked.contains(&"aotriton_attention"));
    }

    #[test]
    fn cpu_acceleration_always_has_scalar_baseline() {
        assert!(super::cpu_acceleration_flags().contains(&"scalar"));
    }
}
