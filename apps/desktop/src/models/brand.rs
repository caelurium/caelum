//! Bundled company marks; model families and connection providers are distinct.
use rune::{El, Icon, TextStyle, col, icon, text};
use rune_kit::Kit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Brand {
    Alibaba,
    Anthropic,
    DeepSeek,
    Google,
    Meta,
    MiniMax,
    Mistral,
    Moonshot,
    OpenAi,
    OpenCode,
    Xai,
    Zai,
}

impl Brand {
    pub(super) fn provider(id: &str) -> Option<Self> {
        Some(match id.to_ascii_lowercase().as_str() {
            "alibaba" | "alibaba-cn" | "qwen" => Self::Alibaba,
            "anthropic" => Self::Anthropic,
            "deepseek" => Self::DeepSeek,
            "google" | "google-vertex" => Self::Google,
            "meta" => Self::Meta,
            "minimax" | "minimax-cn" | "minimax-coding-plan" => Self::MiniMax,
            "mistral" => Self::Mistral,
            "moonshotai" | "moonshotai-cn" | "kimi-for-coding" => Self::Moonshot,
            "openai" | "codex" => Self::OpenAi,
            "opencode" | "opencode-go" => Self::OpenCode,
            "xai" => Self::Xai,
            "zai" | "zhipuai" | "zai-coding-plan" => Self::Zai,
            _ => return None,
        })
    }

    pub(super) fn model(id: &str, name: &str, connection: &str) -> Option<Self> {
        // Inspect the model family before the gateway serving it. Keep unknown
        // custom catalogues under their actual connection instead of guessing.
        for value in [id.rsplit('/').next().unwrap_or(id), name] {
            let value = value.to_ascii_lowercase();
            let brand = if value.starts_with("claude") {
                Some(Self::Anthropic)
            } else if value.starts_with("gpt-")
                || value.starts_with("chatgpt-")
                || ["o1", "o3", "o4"]
                    .iter()
                    .any(|prefix| value == *prefix || value.starts_with(&format!("{prefix}-")))
            {
                Some(Self::OpenAi)
            } else if value.starts_with("gemini") || value.starts_with("gemma") {
                Some(Self::Google)
            } else if value.starts_with("deepseek") {
                Some(Self::DeepSeek)
            } else if value.starts_with("kimi") || value.starts_with("moonshot") {
                Some(Self::Moonshot)
            } else if value.starts_with("minimax") {
                Some(Self::MiniMax)
            } else if value.starts_with("glm") {
                Some(Self::Zai)
            } else if value.starts_with("qwen") || value.starts_with("qwq") {
                Some(Self::Alibaba)
            } else if value.starts_with("llama") {
                Some(Self::Meta)
            } else if ["mistral", "mixtral", "devstral", "codestral", "magistral"]
                .iter()
                .any(|p| value.starts_with(p))
            {
                Some(Self::Mistral)
            } else if value.starts_with("grok") {
                Some(Self::Xai)
            } else {
                None
            };
            if brand.is_some() {
                return brand;
            }
        }
        Self::provider(connection)
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Alibaba => "Alibaba",
            Self::Anthropic => "Anthropic",
            Self::DeepSeek => "DeepSeek",
            Self::Google => "Google",
            Self::Meta => "Meta",
            Self::MiniMax => "MiniMax",
            Self::Mistral => "Mistral",
            Self::Moonshot => "Moonshot AI",
            Self::OpenAi => "OpenAI",
            Self::OpenCode => "OpenCode",
            Self::Xai => "xAI",
            Self::Zai => "Z.ai",
        }
    }

    pub(super) fn glyph(self) -> &'static Icon {
        // Rune reads the path elements directly, so its view must match the
        // source SVG's viewBox (the SVG's width/height are only display sizes).
        macro_rules! mark {
            ($name:literal, $view:literal) => {
                &Icon {
                    name: concat!("model-brand-", $name),
                    view: $view,
                    body: include_str!(concat!("../../assets/models/", $name, ".svg")),
                }
            };
        }
        match self {
            Self::Alibaba => mark!("alibaba", 40.0),
            Self::Anthropic => mark!("anthropic", 40.0),
            Self::DeepSeek => mark!("deepseek", 40.0),
            Self::Google => mark!("google", 40.0),
            Self::Meta => mark!("meta", 40.0),
            Self::MiniMax => mark!("minimax", 40.0),
            Self::Mistral => mark!("mistral", 40.0),
            Self::Moonshot => mark!("moonshotai", 40.0),
            Self::OpenAi => mark!("openai", 40.0),
            Self::OpenCode => mark!("opencode", 24.0),
            Self::Xai => mark!("xai", 40.0),
            Self::Zai => mark!("zai", 40.0),
        }
    }
}

pub(super) fn logo(brand: Option<Brand>, name: &str, size: f32, k: &Kit) -> El {
    if let Some(brand) = brand {
        icon(brand.glyph(), size).color(k.c.text_base).a11y_hidden()
    } else {
        col()
            .size(size, size)
            .center()
            .child(
                text(
                    name.chars()
                        .next()
                        .unwrap_or('?')
                        .to_uppercase()
                        .to_string(),
                )
                .font(TextStyle::new(size * 0.55, 600.0))
                .color(k.c.text_muted),
            )
            .a11y_hidden()
    }
}
