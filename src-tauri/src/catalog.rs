//! ダウンロード可能なモデルと llama.cpp ビルドの一覧

use serde::Serialize;
use std::path::{Path, PathBuf};

/// 動作確認済みの llama.cpp リリース (PLaMo 3 対応済み)
pub const LLAMA_CPP_TAG: &str = "b12144";

#[cfg(windows)]
pub const SERVER_EXE: &str = "llama-server.exe";
#[cfg(not(windows))]
pub const SERVER_EXE: &str = "llama-server";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub size: &'static str,
    pub quant: &'static str,
    pub bytes: u64,
    pub label: &'static str,
}

impl ModelInfo {
    pub fn repo(&self) -> String {
        format!("pfnet/plamo-3-translate-{}-GGUF", self.size)
    }
    pub fn file_name(&self) -> String {
        format!("plamo-3-translate-{}-{}.gguf", self.size, self.quant)
    }
    pub fn url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/main/{}",
            self.repo(),
            self.file_name()
        )
    }
}

pub const MODELS: &[ModelInfo] = &[
    ModelInfo { size: "0.8b", quant: "Q4_K_M", bytes: 573_123_200, label: "標準 (推奨)" },
    ModelInfo { size: "0.8b", quant: "Q4_0", bytes: 547_539_584, label: "Arm 向け" },
    ModelInfo { size: "0.8b", quant: "Q3_K_M", bytes: 486_382_208, label: "省容量" },
    ModelInfo { size: "0.8b", quant: "Q2_K", bytes: 410_135_168, label: "最小" },
    ModelInfo { size: "2b", quant: "Q4_K_M", bytes: 1_807_199_392, label: "標準" },
    ModelInfo { size: "2b", quant: "Q4_0", bytes: 1_698_573_472, label: "Arm 向け" },
    ModelInfo { size: "2b", quant: "Q3_K_M", bytes: 1_495_882_912, label: "省容量" },
    ModelInfo { size: "2b", quant: "Q2_K", bytes: 1_225_608_352, label: "最小" },
    ModelInfo { size: "8b", quant: "Q4_K_M", bytes: 5_280_442_080, label: "標準" },
    ModelInfo { size: "8b", quant: "Q3_K_M", bytes: 4_310_214_368, label: "省容量" },
    ModelInfo { size: "8b", quant: "Q2_K", bytes: 3_450_742_496, label: "最小" },
];

pub fn find_model(size: &str, quant: &str) -> Option<&'static ModelInfo> {
    MODELS.iter().find(|m| m.size == size && m.quant == quant)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendInfo {
    pub id: &'static str,
    pub label: &'static str,
    /// llama.cpp リリースのアセット名 ({tag} は LLAMA_CPP_TAG に置換)
    #[serde(skip)]
    pub assets: &'static [&'static str],
    pub approx_bytes: u64,
}

impl BackendInfo {
    pub fn urls(&self) -> Vec<(String, String)> {
        self.assets
            .iter()
            .map(|a| {
                let name = a.replace("{tag}", LLAMA_CPP_TAG);
                (
                    format!(
                        "https://github.com/ggml-org/llama.cpp/releases/download/{}/{}",
                        LLAMA_CPP_TAG, name
                    ),
                    name,
                )
            })
            .collect()
    }
}

pub const BACKENDS: &[BackendInfo] = &[
    BackendInfo {
        id: "vulkan",
        label: "GPU (Vulkan) - NVIDIA / AMD / Intel 対応",
        assets: &["llama-{tag}-bin-win-vulkan-x64.zip"],
        approx_bytes: 37_700_000,
    },
    BackendInfo {
        id: "cpu",
        label: "CPU のみ",
        assets: &["llama-{tag}-bin-win-cpu-x64.zip"],
        approx_bytes: 24_700_000,
    },
    BackendInfo {
        id: "cuda-12.4",
        label: "GPU (CUDA 12.4) - NVIDIA",
        assets: &[
            "llama-{tag}-bin-win-cuda-12.4-x64.zip",
            "cudart-llama-bin-win-cuda-12.4-x64.zip",
        ],
        approx_bytes: 680_000_000,
    },
    BackendInfo {
        id: "cuda-13.4",
        label: "GPU (CUDA 13.4) - NVIDIA",
        assets: &[
            "llama-{tag}-bin-win-cuda-13.4-x64.zip",
            "cudart-llama-bin-win-cuda-13.4-x64.zip",
        ],
        approx_bytes: 594_000_000,
    },
];

pub fn find_backend(id: &str) -> Option<&'static BackendInfo> {
    BACKENDS.iter().find(|b| b.id == id)
}

pub fn backend_dir(data_dir: &Path, backend: &str) -> PathBuf {
    data_dir
        .join("llama.cpp")
        .join(format!("{}-{}", LLAMA_CPP_TAG, backend))
}
