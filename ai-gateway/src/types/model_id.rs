use std::{
    borrow::Cow,
    fmt::{self, Display},
    str::FromStr,
};

use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use derive_more::AsRef;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::provider::InferenceProvider;
use crate::error::mapper::MapperError;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Version {
    /// Same as `Latest` but without the `latest` suffix.
    ImplicitLatest,
    /// An alias for the latest version of the model.
    Latest,
    /// An alias for the latest preview version of the model.
    Preview,
    /// A specific version of a preview model based on the date it was released.
    DateVersionedPreview {
        date: DateTime<Utc>,
        /// The format of the date so we know how to re-serialize it
        format: &'static str,
    },
    /// A version of the model based on the date it was released.
    Date {
        date: DateTime<Utc>,
        /// The format of the date so we know how to re-serialize it
        format: &'static str,
    },
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Version::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl Serialize for Version {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Version::ImplicitLatest => write!(f, ""),
            Version::Latest => write!(f, "latest"),
            Version::Preview => write!(f, "preview"),
            Version::DateVersionedPreview { date, format } => {
                write!(f, "preview-{}", date.format(format))
            }
            Version::Date { date, format } => {
                write!(f, "{}", date.format(format))
            }
        }
    }
}

impl FromStr for Version {
    type Err = MapperError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        if input.eq_ignore_ascii_case("latest") {
            Ok(Version::Latest)
        } else if input.eq_ignore_ascii_case("preview") {
            Ok(Version::Preview)
        } else if let Some(rest) = input.strip_prefix("preview-") {
            if let Some((dt, fmt)) = parse_date(rest) {
                Ok(Version::DateVersionedPreview {
                    date: dt,
                    format: fmt,
                })
            } else {
                Err(MapperError::InvalidModelName(input.to_string()))
            }
        } else if let Some((dt, fmt)) = parse_date(input) {
            Ok(Version::Date {
                date: dt,
                format: fmt,
            })
        } else if input.is_empty() {
            Ok(Version::ImplicitLatest)
        } else {
            Err(MapperError::InvalidModelName(input.to_string()))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, AsRef, Serialize, Deserialize)]
pub struct ModelName<'a>(Cow<'a, str>);

impl<'a> ModelName<'a> {
    #[must_use]
    pub fn borrowed(name: &'a str) -> Self {
        Self(Cow::Borrowed(name))
    }

    #[must_use]
    pub fn owned(name: String) -> Self {
        Self(Cow::Owned(name))
    }

    #[must_use]
    pub fn from_model(model: &'a ModelId) -> Self {
        match model {
            ModelId::ModelIdWithVersion { id, .. } => {
                Self(Cow::Borrowed(id.model.as_str()))
            }
            ModelId::Bedrock(bedrock_model_id) => {
                Self(Cow::Borrowed(bedrock_model_id.model.as_str()))
            }
            ModelId::Ollama(ollama_model_id) => {
                Self(Cow::Borrowed(ollama_model_id.model.as_str()))
            }
            ModelId::Unknown(model_id) => {
                Self(Cow::Borrowed(model_id.as_str()))
            }
        }
    }
}

impl std::fmt::Display for ModelName<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ModelId {
    ModelIdWithVersion {
        provider: InferenceProvider,
        id: ModelIdWithVersion,
    },
    Bedrock(BedrockModelId),
    Ollama(OllamaModelId),
    Unknown(String),
}

impl ModelId {
    /// Create a `ModelId` from a string and an inference provider.
    ///
    /// The `request_style` parameter here is used to determine what format
    /// the model name is in.
    pub(crate) fn from_str_and_provider(
        request_style: InferenceProvider,
        s: &str,
    ) -> Result<Self, MapperError> {
        match request_style {
            InferenceProvider::OpenAI => {
                let model_with_version = ModelIdWithVersion::from_str(s)?;
                Ok(ModelId::ModelIdWithVersion {
                    provider: InferenceProvider::OpenAI,
                    id: model_with_version,
                })
            }
            InferenceProvider::Anthropic => {
                let model_with_version = ModelIdWithVersion::from_str(s)?;
                Ok(ModelId::ModelIdWithVersion {
                    provider: InferenceProvider::Anthropic,
                    id: model_with_version,
                })
            }
            InferenceProvider::Bedrock => {
                let bedrock_model = BedrockModelId::from_str(s)?;
                Ok(ModelId::Bedrock(bedrock_model))
            }
            InferenceProvider::Ollama => {
                let ollama_model = OllamaModelId::from_str(s)?;
                Ok(ModelId::Ollama(ollama_model))
            }
            InferenceProvider::GoogleGemini => {
                let model_with_version = ModelIdWithVersion::from_str(s)?;
                Ok(ModelId::ModelIdWithVersion {
                    provider: InferenceProvider::GoogleGemini,
                    id: model_with_version,
                })
            }
            InferenceProvider::Named(name) => {
                let model_with_version = ModelIdWithVersion::from_str(s)?;
                Ok(ModelId::ModelIdWithVersion {
                    provider: InferenceProvider::Named(name),
                    id: model_with_version,
                })
            }
        }
    }

    #[must_use]
    pub fn inference_provider(&self) -> Option<InferenceProvider> {
        match self {
            ModelId::ModelIdWithVersion { provider, .. } => {
                Some(provider.clone())
            }
            ModelId::Bedrock(_) => Some(InferenceProvider::Bedrock),
            ModelId::Ollama(_) => Some(InferenceProvider::Ollama),
            ModelId::Unknown(_) => None,
        }
    }

    #[must_use]
    pub fn as_model_name(&self) -> ModelName<'_> {
        match self {
            ModelId::ModelIdWithVersion { id, .. } => {
                ModelName::borrowed(id.model.as_str())
            }
            ModelId::Bedrock(model) => {
                ModelName::borrowed(model.model.as_str())
            }
            ModelId::Ollama(model) => ModelName::borrowed(model.model.as_str()),
            ModelId::Unknown(model) => ModelName::borrowed(model),
        }
    }

    #[must_use]
    pub fn as_model_name_owned(&self) -> ModelName<'static> {
        match self {
            ModelId::ModelIdWithVersion { id, .. } => {
                ModelName::owned(id.model.clone())
            }
            ModelId::Bedrock(model) => ModelName::owned(model.model.clone()),
            ModelId::Ollama(model) => ModelName::owned(model.model.clone()),
            ModelId::Unknown(model) => ModelName::owned(model.clone()),
        }
    }

    #[must_use]
    pub fn with_latest_version(self) -> ModelId {
        match self {
            ModelId::ModelIdWithVersion { provider, id } => {
                ModelId::ModelIdWithVersion {
                    provider,
                    id: ModelIdWithVersion {
                        model: id.model,
                        version: Version::Latest,
                    },
                }
            }
            ModelId::Bedrock(bedrock_model_id) => {
                ModelId::Bedrock(BedrockModelId {
                    geo: bedrock_model_id.geo,
                    provider: bedrock_model_id.provider,
                    model: bedrock_model_id.model,
                    version: Version::Latest,
                    bedrock_internal_version: bedrock_model_id
                        .bedrock_internal_version,
                })
            }
            ModelId::Ollama(ollama_model_id) => {
                ModelId::Ollama(OllamaModelId {
                    model: ollama_model_id.model,
                    tag: ollama_model_id.tag,
                })
            }
            ModelId::Unknown(model) => ModelId::Unknown(model),
        }
    }
}

impl<'de> Deserialize<'de> for ModelId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        ModelId::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl Serialize for ModelId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(provider) = self.inference_provider() {
            serializer.serialize_str(&format!("{provider}/{self}"))
        } else {
            serializer.serialize_str(&self.to_string())
        }
    }
}

/// Parse a model id in the format `{provider}/{model_name}` to a `ModelId`.
impl FromStr for ModelId {
    type Err = MapperError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(2, '/');
        let provider_str = parts.next();
        let model_name = parts.next();

        match (provider_str, model_name) {
            (Some(provider_str), Some(model_name)) => {
                if model_name.is_empty() {
                    return Err(MapperError::InvalidModelName(
                        "Model name cannot be empty after provider".to_string(),
                    ));
                }

                let provider = InferenceProvider::from_str(provider_str)
                    .map_err(|_| {
                        MapperError::ProviderNotSupported(
                            provider_str.to_string(),
                        )
                    })?;

                Self::from_str_and_provider(provider, model_name)
            }
            _ => Err(MapperError::InvalidModelName(format!(
                "Model string must be in format \
                 '{{provider}}/{{model_name}}', got '{s}'",
            ))),
        }
    }
}

impl Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelId::ModelIdWithVersion { id, .. } => id.fmt(f),
            ModelId::Bedrock(model) => model.fmt(f),
            ModelId::Ollama(model) => model.fmt(f),
            ModelId::Unknown(model) => model.fmt(f),
        }
    }
}

impl From<ModelId> for ModelIdWithoutVersion {
    fn from(model_id: ModelId) -> Self {
        Self { inner: model_id }
    }
}

impl PartialEq for ModelIdWithoutVersion {
    fn eq(&self, other: &Self) -> bool {
        match (&self.inner, &other.inner) {
            (
                ModelId::ModelIdWithVersion { provider, id },
                ModelId::ModelIdWithVersion {
                    provider: other_provider,
                    id: other_id,
                },
            ) => provider == other_provider && id.model == other_id.model,
            (ModelId::Bedrock(this), ModelId::Bedrock(other)) => {
                this.provider == other.provider
                    && this.model == other.model
                    && this.bedrock_internal_version
                        == other.bedrock_internal_version
            }
            (ModelId::Ollama(this), ModelId::Ollama(other)) => {
                this.model == other.model && this.tag == other.tag
            }
            (ModelId::Unknown(this), ModelId::Unknown(other)) => this == other,
            (
                ModelId::ModelIdWithVersion { .. }
                | ModelId::Bedrock(_)
                | ModelId::Ollama(_)
                | ModelId::Unknown(_),
                _,
            ) => false,
        }
    }
}

impl std::hash::Hash for ModelIdWithoutVersion {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match &self.inner {
            ModelId::ModelIdWithVersion { provider, id } => {
                provider.hash(state);
                id.model.hash(state);
            }
            ModelId::Bedrock(bedrock_model_id) => {
                bedrock_model_id.provider.hash(state);
                bedrock_model_id.model.hash(state);
                bedrock_model_id.bedrock_internal_version.hash(state);
            }
            ModelId::Ollama(ollama_model_id) => {
                ollama_model_id.model.hash(state);
                ollama_model_id.tag.hash(state);
            }
            ModelId::Unknown(model) => model.hash(state),
        }
    }
}

#[derive(Debug, Clone, Eq)]
pub struct ModelIdWithoutVersion {
    inner: ModelId,
}

/// Has the format of: `{model}-{version}`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelIdWithVersion {
    pub model: String,
    pub version: Version,
}

impl FromStr for ModelIdWithVersion {
    type Err = MapperError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Validate input string
        if s.is_empty() {
            return Err(MapperError::InvalidModelName(
                "Model name cannot be empty".to_string(),
            ));
        }

        if s.ends_with('-') {
            return Err(MapperError::InvalidModelName(
                "Model name cannot end with dash".to_string(),
            ));
        }

        if s.ends_with('.') {
            return Err(MapperError::InvalidModelName(
                "Model name cannot end with dot".to_string(),
            ));
        }

        if s.ends_with('@') {
            return Err(MapperError::InvalidModelName(
                "Model name cannot end with @ symbol".to_string(),
            ));
        }

        let (model, version) = parse_model_and_version(s, '-');
        Ok(ModelIdWithVersion {
            model: model.to_string(),
            version: version.unwrap_or(Version::ImplicitLatest),
        })
    }
}

impl Display for ModelIdWithVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.version {
            Version::ImplicitLatest => write!(f, "{}", self.model),
            _ => write!(f, "{}-{}", self.model, self.version),
        }
    }
}

impl<'de> Deserialize<'de> for ModelIdWithVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        ModelIdWithVersion::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl Serialize for ModelIdWithVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// Has the format of: `{model}:{tag}`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OllamaModelId {
    pub model: String,
    pub tag: Option<String>,
}

impl FromStr for OllamaModelId {
    type Err = MapperError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(2, ':');
        let model = parts
            .next()
            .ok_or_else(|| MapperError::InvalidModelName(s.to_string()))?;
        let tag = parts.next();
        Ok(OllamaModelId {
            model: model.to_string(),
            tag: match tag {
                Some(t) if !t.is_empty() => Some(t.to_string()),
                _ => None,
            },
        })
    }
}

impl Display for OllamaModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.tag {
            Some(tag) => write!(f, "{}:{}", self.model, tag),
            None => write!(f, "{}", self.model),
        }
    }
}

impl<'de> Deserialize<'de> for OllamaModelId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        OllamaModelId::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl Serialize for OllamaModelId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// Has the format of:
/// `{geo}?.{provider}.{model}(-version)?-{bedrock_internal_version}`
/// amazon.nova-pro-v1:0
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BedrockModelId {
    pub geo: Option<String>,
    pub provider: String,
    pub model: String,
    pub version: Version,
    pub bedrock_internal_version: String,
}

impl FromStr for BedrockModelId {
    type Err = MapperError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Count the number of dots to determine if geo is present
        let dot_count = s.chars().filter(|&c| c == '.').count();

        let (geo, provider_str, rest) = if dot_count >= 2 {
            // Format: {geo}.{provider}.{model}(-version)?
            // -{bedrock_internal_version}
            let mut parts = s.splitn(3, '.');
            let geo = parts
                .next()
                .ok_or_else(|| MapperError::InvalidModelName(s.to_string()))?;
            let provider = parts
                .next()
                .ok_or_else(|| MapperError::InvalidModelName(s.to_string()))?;
            let rest = parts
                .next()
                .ok_or_else(|| MapperError::InvalidModelName(s.to_string()))?;
            (Some(geo.to_string()), provider, rest)
        } else if dot_count == 1 {
            // Format: {provider}.{model}(-version)?-{bedrock_internal_version}
            let mut parts = s.splitn(2, '.');
            let provider = parts
                .next()
                .ok_or_else(|| MapperError::InvalidModelName(s.to_string()))?;
            let rest = parts
                .next()
                .ok_or_else(|| MapperError::InvalidModelName(s.to_string()))?;
            (None, provider, rest)
        } else {
            return Err(MapperError::InvalidModelName(s.to_string()));
        };

        // Parse the bedrock internal version
        // eg: claude-3-sonnet-20240229-v1:0 (split on `-v`)
        let (model_part, bedrock_version) =
            if let Some(v_pos) = rest.rfind("-v") {
                (&rest[..v_pos], &rest[v_pos + 1..]) // +1 to skip the '-', keeping 'v1:0'
            } else {
                return Err(MapperError::InvalidModelName(s.to_string()));
            };

        // Parse the model and version from the model_part
        let (model, version) = parse_model_and_version(model_part, '-');

        Ok(BedrockModelId {
            geo,
            provider: provider_str.to_string(),
            model: model.to_string(),
            version: version.unwrap_or(Version::ImplicitLatest),
            bedrock_internal_version: bedrock_version.to_string(),
        })
    }
}

impl Display for BedrockModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.geo, &self.version) {
            (Some(geo), Version::ImplicitLatest) => write!(
                f,
                "{}.{}.{}-{}",
                geo, self.provider, self.model, self.bedrock_internal_version
            ),
            (Some(geo), version) => write!(
                f,
                "{}.{}.{}-{}-{}",
                geo,
                self.provider,
                self.model,
                version,
                self.bedrock_internal_version
            ),

            (None, Version::ImplicitLatest) => write!(
                f,
                "{}.{}-{}",
                self.provider, self.model, self.bedrock_internal_version
            ),
            (None, version) => write!(
                f,
                "{}.{}-{}-{}",
                self.provider,
                self.model,
                version,
                self.bedrock_internal_version
            ),
        }
    }
}

impl<'de> Deserialize<'de> for BedrockModelId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        BedrockModelId::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl Serialize for BedrockModelId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

fn parse_date(input: &str) -> Option<(DateTime<Utc>, &'static str)> {
    // try YYYY-MM-DD first
    if let Ok(date) = NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        if let Some(naive_dt) = date.and_hms_opt(0, 0, 0) {
            return Some((Utc.from_utc_datetime(&naive_dt), "%Y-%m-%d"));
        }
    }
    // then YYYYMMDD
    if let Ok(date) = NaiveDate::parse_from_str(input, "%Y%m%d") {
        if let Some(naive_dt) = date.and_hms_opt(0, 0, 0) {
            return Some((Utc.from_utc_datetime(&naive_dt), "%Y%m%d"));
        }
    }
    // then MM-DD (assume current year)
    if let Ok(date) = NaiveDate::parse_from_str(
        &format!("{}-{}", chrono::Utc::now().year(), input),
        "%Y-%m-%d",
    ) {
        if let Some(naive_dt) = date.and_hms_opt(0, 0, 0) {
            return Some((Utc.from_utc_datetime(&naive_dt), "%m-%d"));
        }
    }
    // then MMDD (assume current year)
    if let Ok(date) = NaiveDate::parse_from_str(
        &format!("{}{}", chrono::Utc::now().year(), input),
        "%Y%m%d",
    ) {
        if let Some(naive_dt) = date.and_hms_opt(0, 0, 0) {
            return Some((Utc.from_utc_datetime(&naive_dt), "%m%d"));
        }
    }
    None
}

fn parse_model_and_version(
    s: &str,
    separator: char,
) -> (&str, Option<Version>) {
    // Handle special case for preview versions with dates first
    if let Some(preview_pos) = s.rfind("-preview-") {
        let after_preview = &s[preview_pos + 9..]; // 9 = length of "-preview-"
        if let Some((dt, fmt)) = parse_date(after_preview) {
            let model = &s[..preview_pos];
            return (
                model,
                Some(Version::DateVersionedPreview {
                    date: dt,
                    format: fmt,
                }),
            );
        }
    }

    // Handle "preview" version (not date-versioned)
    if let Some(model) = s.strip_suffix("-preview") {
        return (model, Some(Version::Preview));
    }

    // Handle "latest" version
    if let Some(model) = s.strip_suffix("-latest") {
        return (model, Some(Version::Latest));
    }

    // Collect all possible version candidates first, then choose the best one
    let mut candidates = Vec::new();
    for (idx, ch) in s.char_indices().rev() {
        if ch == separator {
            // Check for trailing separator
            if idx == s.len() - 1 {
                continue;
            }
            let candidate = &s[idx + 1..];
            candidates.push((idx, candidate));
        }
    }

    // Reverse to check longer candidates first (leftmost separators first)
    candidates.reverse();

    // Try to find the best version candidate, prioritizing longer dates
    for (idx, candidate) in &candidates {
        // Try parsing as date first (prioritize full dates like YYYY-MM-DD over
        // MM-DD)
        if let Some((dt, fmt)) = parse_date(candidate) {
            // Prefer YYYY-MM-DD and YYYYMMDD formats over MM-DD
            if fmt == "%Y-%m-%d" || fmt == "%Y%m%d" {
                let model = &s[..*idx];
                return (
                    model,
                    Some(Version::Date {
                        date: dt,
                        format: fmt,
                    }),
                );
            }
        }
    }

    // If no full date found, try other version types
    for (idx, candidate) in &candidates {
        // Try parsing as date (including MM-DD)
        if let Some((dt, fmt)) = parse_date(candidate) {
            let model = &s[..*idx];
            return (
                model,
                Some(Version::Date {
                    date: dt,
                    format: fmt,
                }),
            );
        }

        // Try parsing as special version keywords
        if candidate.eq_ignore_ascii_case("latest") {
            let model = &s[..*idx];
            return (model, Some(Version::Latest));
        } else if candidate.eq_ignore_ascii_case("preview") {
            let model = &s[..*idx];
            return (model, Some(Version::Preview));
        }
    }

    // No valid version found
    (s, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groq_model_id_format_with_slash() {
        let groq_model_id_str = "meta-llama/llama-4-maverick-17b-128e-instruct";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Named("groq".into()),
            groq_model_id_str,
        )
        .unwrap();
        let ModelId::ModelIdWithVersion { provider, id } = result else {
            panic!("Expected ModelIdWithVersion with Groq provider");
        };
        assert_eq!(
            id,
            ModelIdWithVersion {
                model: "meta-llama/llama-4-maverick-17b-128e-instruct"
                    .to_string(),
                version: Version::ImplicitLatest,
            }
        );
        assert_eq!(provider, InferenceProvider::Named("groq".into()));
    }

    #[test]
    fn groq_model_id_format_without_slash() {
        let groq_model_id_str = "deepseek-r1-distill-llama-70b";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Named("groq".into()),
            groq_model_id_str,
        )
        .unwrap();
        let ModelId::ModelIdWithVersion { provider, id } = result else {
            panic!("Expected ModelIdWithVersion with Groq provider");
        };
        assert_eq!(
            id,
            ModelIdWithVersion {
                model: "deepseek-r1-distill-llama-70b".to_string(),
                version: Version::ImplicitLatest,
            }
        );
        assert_eq!(provider, InferenceProvider::Named("groq".into()));
    }

    #[test]
    fn test_openai_o1_snapshot_model() {
        let model_id_str = "o1-2024-12-17";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::OpenAI,
            model_id_str,
        )
        .unwrap();
        let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = &result
        else {
            panic!("Expected ModelIdWithVersion with OpenAI provider");
        };
        assert!(matches!(provider, InferenceProvider::OpenAI));
        assert_eq!(model_with_version.model, "o1");
        let Version::Date { date, .. } = &model_with_version.version else {
            panic!("Expected date version");
        };
        let expected_dt: DateTime<Utc> =
            "2024-12-17T00:00:00Z".parse().unwrap();
        assert_eq!(*date, expected_dt);

        assert_eq!(result.to_string(), model_id_str);
    }

    #[test]
    fn test_openai_o1_preview_snapshot_model() {
        let model_id_str = "o1-preview-2024-09-12";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::OpenAI,
            model_id_str,
        )
        .unwrap();
        let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = &result
        else {
            panic!("Expected ModelIdWithVersion with OpenAI provider");
        };
        assert!(matches!(provider, InferenceProvider::OpenAI));
        assert_eq!(model_with_version.model, "o1");
        let Version::DateVersionedPreview { date, .. } =
            &model_with_version.version
        else {
            panic!("Expected date versioned preview");
        };
        let expected_dt: DateTime<Utc> =
            "2024-09-12T00:00:00Z".parse().unwrap();
        assert_eq!(*date, expected_dt);

        assert_eq!(result.to_string(), model_id_str);
    }

    #[test]
    fn test_openai_gpt4_snapshot_model() {
        let model_id_str = "gpt-4-2024-08-15";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::OpenAI,
            model_id_str,
        )
        .unwrap();
        let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = &result
        else {
            panic!("Expected ModelIdWithVersion with OpenAI provider");
        };
        assert!(matches!(provider, InferenceProvider::OpenAI));
        assert_eq!(model_with_version.model, "gpt-4");
        let Version::Date { date, .. } = &model_with_version.version else {
            panic!("Expected date version");
        };
        let expected_dt: DateTime<Utc> =
            "2024-08-15T00:00:00Z".parse().unwrap();
        assert_eq!(*date, expected_dt);

        assert_eq!(result.to_string(), model_id_str);
    }

    #[test]
    fn test_openai_gpt35_turbo_snapshot_model() {
        let model_id_str = "gpt-3.5-turbo-2024-01-25";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::OpenAI,
            model_id_str,
        )
        .unwrap();
        let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = &result
        else {
            panic!("Expected ModelIdWithVersion with OpenAI provider");
        };
        assert!(matches!(provider, InferenceProvider::OpenAI));
        assert_eq!(model_with_version.model, "gpt-3.5-turbo");
        let Version::Date { date, .. } = &model_with_version.version else {
            panic!("Expected date version");
        };
        let expected_dt: DateTime<Utc> =
            "2024-01-25T00:00:00Z".parse().unwrap();
        assert_eq!(*date, expected_dt);

        assert_eq!(result.to_string(), model_id_str);
    }

    #[test]
    fn test_openai_o1_alias_model() {
        let model_id_str = "o1";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::OpenAI,
            model_id_str,
        )
        .unwrap();
        let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = &result
        else {
            panic!("Expected ModelIdWithVersion with OpenAI provider");
        };
        assert!(matches!(provider, InferenceProvider::OpenAI));
        assert_eq!(model_with_version.model, "o1");
        assert!(matches!(
            model_with_version.version,
            Version::ImplicitLatest
        ));

        assert_eq!(result.to_string(), model_id_str);
    }

    #[test]
    fn test_openai_o1_preview_alias_model() {
        let model_id_str = "o1-preview";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::OpenAI,
            model_id_str,
        )
        .unwrap();
        let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = &result
        else {
            panic!("Expected ModelIdWithVersion with OpenAI provider");
        };
        assert!(matches!(provider, InferenceProvider::OpenAI));
        assert_eq!(model_with_version.model, "o1");
        assert!(matches!(model_with_version.version, Version::Preview));

        assert_eq!(result.to_string(), model_id_str);
    }

    #[test]
    fn test_openai_gpt4_alias_model() {
        let model_id_str = "gpt-4";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::OpenAI,
            model_id_str,
        )
        .unwrap();
        let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = &result
        else {
            panic!("Expected ModelIdWithVersion with OpenAI provider");
        };
        assert!(matches!(provider, InferenceProvider::OpenAI));
        assert_eq!(model_with_version.model, "gpt-4");
        assert!(matches!(
            model_with_version.version,
            Version::ImplicitLatest
        ));

        assert_eq!(result.to_string(), model_id_str);
    }

    #[test]
    fn test_openai_gpt35_turbo_alias_model() {
        let model_id_str = "gpt-3.5-turbo";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::OpenAI,
            model_id_str,
        )
        .unwrap();
        let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = &result
        else {
            panic!("Expected ModelIdWithVersion with OpenAI provider");
        };
        assert!(matches!(provider, InferenceProvider::OpenAI));
        assert_eq!(model_with_version.model, "gpt-3.5-turbo");
        assert!(matches!(
            model_with_version.version,
            Version::ImplicitLatest
        ));

        assert_eq!(result.to_string(), model_id_str);
    }

    #[test]
    fn test_anthropic_claude_opus_4_dated_model() {
        let model_id_str = "claude-opus-4-20250514";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Anthropic,
            model_id_str,
        )
        .unwrap();
        let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = &result
        else {
            panic!("Expected ModelIdWithVersion with Anthropic provider");
        };
        assert!(matches!(provider, InferenceProvider::Anthropic));
        assert_eq!(model_with_version.model, "claude-opus-4");
        let Version::Date { date, .. } = model_with_version.version else {
            panic!("Expected date version");
        };
        let expected_dt: DateTime<Utc> =
            "2025-05-14T00:00:00Z".parse().unwrap();
        assert_eq!(date, expected_dt);

        assert_eq!(result.to_string(), model_id_str);
    }

    #[test]
    fn test_anthropic_claude_sonnet_4_dated_model() {
        let model_id_str = "claude-sonnet-4-20250514";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Anthropic,
            model_id_str,
        )
        .unwrap();
        let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = &result
        else {
            panic!("Expected ModelIdWithVersion with Anthropic provider");
        };
        assert!(matches!(provider, InferenceProvider::Anthropic));
        assert_eq!(model_with_version.model, "claude-sonnet-4");
        let Version::Date { date, .. } = &model_with_version.version else {
            panic!("Expected date version");
        };
        let expected_dt: DateTime<Utc> =
            "2025-05-14T00:00:00Z".parse().unwrap();
        assert_eq!(*date, expected_dt);

        assert_eq!(result.to_string(), model_id_str);
    }

    #[test]
    fn test_anthropic_claude_3_7_sonnet_dated_model() {
        let model_id_str = "claude-3-7-sonnet-20250219";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Anthropic,
            model_id_str,
        )
        .unwrap();
        let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = &result
        else {
            panic!("Expected ModelIdWithVersion with Anthropic provider");
        };
        assert!(matches!(provider, InferenceProvider::Anthropic));
        assert_eq!(model_with_version.model, "claude-3-7-sonnet");
        let Version::Date { date, .. } = &model_with_version.version else {
            panic!("Expected date version");
        };
        let expected_dt: DateTime<Utc> =
            "2025-02-19T00:00:00Z".parse().unwrap();
        assert_eq!(*date, expected_dt);

        assert_eq!(result.to_string(), model_id_str);
    }

    #[test]
    fn test_anthropic_claude_3_haiku_dated_model() {
        let model_id_str = "claude-3-haiku-20240307";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Anthropic,
            model_id_str,
        )
        .unwrap();
        let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = &result
        else {
            panic!("Expected ModelIdWithVersion with Anthropic provider");
        };
        assert!(matches!(provider, InferenceProvider::Anthropic));
        assert_eq!(model_with_version.model, "claude-3-haiku");
        let Version::Date { date, .. } = &model_with_version.version else {
            panic!("Expected date version");
        };
        let expected_dt: DateTime<Utc> =
            "2024-03-07T00:00:00Z".parse().unwrap();
        assert_eq!(*date, expected_dt);

        assert_eq!(result.to_string(), model_id_str);
    }

    #[test]
    fn test_anthropic_claude_3_7_sonnet_latest_alias() {
        let model_id_str = "claude-3-7-sonnet-latest";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Anthropic,
            model_id_str,
        )
        .unwrap();
        let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = &result
        else {
            panic!("Expected ModelIdWithVersion with Anthropic provider");
        };
        assert!(matches!(provider, InferenceProvider::Anthropic));
        assert_eq!(model_with_version.model, "claude-3-7-sonnet");
        assert!(matches!(model_with_version.version, Version::Latest));

        // Display for Version::Latest appends "-latest"
        assert_eq!(result.to_string(), "claude-3-7-sonnet-latest");
    }

    #[test]
    fn test_anthropic_claude_sonnet_4_latest_alias() {
        let model_id_str = "claude-sonnet-4-latest";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Anthropic,
            model_id_str,
        )
        .unwrap();
        let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = &result
        else {
            panic!("Expected ModelIdWithVersion with Anthropic provider");
        };
        assert!(matches!(provider, InferenceProvider::Anthropic));
        assert_eq!(model_with_version.model, "claude-sonnet-4");
        assert!(matches!(model_with_version.version, Version::Latest));

        // Display for Version::Latest appends "-latest"
        assert_eq!(result.to_string(), "claude-sonnet-4-latest");
    }

    #[test]
    fn test_anthropic_claude_opus_4_0_implicit_latest() {
        let model_id_str = "claude-opus-4-0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Anthropic,
            model_id_str,
        )
        .unwrap();
        let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = &result
        else {
            panic!("Expected ModelIdWithVersion with Anthropic provider");
        };
        assert!(matches!(provider, InferenceProvider::Anthropic));
        assert_eq!(model_with_version.model, "claude-opus-4-0");
        assert!(matches!(
            model_with_version.version,
            Version::ImplicitLatest
        ));

        assert_eq!(result.to_string(), model_id_str);
    }

    #[test]
    fn test_anthropic_claude_sonnet_4_0_implicit_latest() {
        let model_id_str = "claude-sonnet-4-0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Anthropic,
            model_id_str,
        )
        .unwrap();
        let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = &result
        else {
            panic!("Expected ModelIdWithVersion with Anthropic provider");
        };
        assert!(matches!(provider, InferenceProvider::Anthropic));
        assert_eq!(model_with_version.model, "claude-sonnet-4-0");
        assert!(matches!(
            model_with_version.version,
            Version::ImplicitLatest
        ));

        assert_eq!(result.to_string(), model_id_str);
    }

    #[test]
    fn test_bedrock_amazon_titan_valid_provider() {
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            "amazon.titan-embed-text-v1:0",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_bedrock_ai21_jamba_valid_provider() {
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            "ai21.jamba-1-5-large-v1:0",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_bedrock_meta_llama_valid_provider() {
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            "meta.llama3-8b-instruct-v1:0",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_bedrock_openai_invalid_format() {
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            "openai.gpt-4:1",
        );
        assert!(result.is_err());
        // This should fail because the format doesn't have `-v` pattern
        // required for Bedrock
        if let Err(MapperError::InvalidModelName(model_name)) = result {
            assert_eq!(model_name, "openai.gpt-4:1");
        } else {
            panic!(
                "Expected InvalidModelName error for OpenAI format on \
                 Bedrock, got: {result:?}"
            );
        }
    }

    #[test]
    fn test_bedrock_anthropic_claude_opus_4_model() {
        let model_id_str = "anthropic.claude-opus-4-20250514-v1:0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(
                bedrock_model.provider,
                InferenceProvider::Anthropic.to_string()
            );
            assert_eq!(bedrock_model.model, "claude-opus-4");
            let Version::Date { date, .. } = &bedrock_model.version else {
                panic!("Expected date version");
            };
            let expected_dt: chrono::DateTime<chrono::Utc> =
                "2025-05-14T00:00:00Z".parse().unwrap();
            assert_eq!(*date, expected_dt);
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");

            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Bedrock ModelId with Anthropic provider");
        }
    }

    #[test]
    fn test_bedrock_anthropic_claude_3_7_sonnet_model() {
        let model_id_str = "anthropic.claude-3-7-sonnet-20250219-v1:0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(
                bedrock_model.provider,
                InferenceProvider::Anthropic.to_string()
            );
            assert_eq!(bedrock_model.model, "claude-3-7-sonnet");
            let Version::Date { date, .. } = &bedrock_model.version else {
                panic!("Expected date version");
            };
            let expected_dt: chrono::DateTime<chrono::Utc> =
                "2025-02-19T00:00:00Z".parse().unwrap();
            assert_eq!(*date, expected_dt);
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Bedrock ModelId with Anthropic provider");
        }
    }

    #[test]
    fn test_bedrock_anthropic_claude_3_haiku_model() {
        let model_id_str = "anthropic.claude-3-haiku-20240307-v1:0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(
                bedrock_model.provider,
                InferenceProvider::Anthropic.to_string()
            );
            assert_eq!(bedrock_model.model, "claude-3-haiku");
            let Version::Date { date, .. } = &bedrock_model.version else {
                panic!("Expected date version");
            };
            let expected_dt: chrono::DateTime<chrono::Utc> =
                "2024-03-07T00:00:00Z".parse().unwrap();
            assert_eq!(*date, expected_dt);
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Bedrock ModelId with Anthropic provider");
        }
    }

    #[test]
    fn test_bedrock_anthropic_claude_3_sonnet_valid_provider() {
        let model_id_str = "anthropic.claude-3-sonnet-20240229-v1:0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(
                bedrock_model.provider,
                InferenceProvider::Anthropic.to_string()
            );
            assert_eq!(bedrock_model.model, "claude-3-sonnet");
            let Version::Date { date, .. } = &bedrock_model.version else {
                panic!("Expected date version");
            };
            let expected_dt: chrono::DateTime<chrono::Utc> =
                "2024-02-29T00:00:00Z".parse().unwrap();
            assert_eq!(*date, expected_dt);
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Bedrock ModelId with Anthropic provider");
        }
    }

    #[test]
    fn test_bedrock_anthropic_claude_3_5_sonnet_model() {
        let model_id_str = "anthropic.claude-3-5-sonnet-20241022-v2:0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(
                bedrock_model.provider,
                InferenceProvider::Anthropic.to_string()
            );
            assert_eq!(bedrock_model.model, "claude-3-5-sonnet");
            let Version::Date { date, .. } = &bedrock_model.version else {
                panic!("Expected date version");
            };
            let expected_dt: chrono::DateTime<chrono::Utc> =
                "2024-10-22T00:00:00Z".parse().unwrap();
            assert_eq!(*date, expected_dt);
            assert_eq!(bedrock_model.bedrock_internal_version, "v2:0");
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Bedrock ModelId with Anthropic provider");
        }
    }

    #[test]
    fn test_bedrock_anthropic_claude_sonnet_4_model_proper_format() {
        let model_id_str = "anthropic.claude-sonnet-4-20250514-v1:0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(
                bedrock_model.provider,
                InferenceProvider::Anthropic.to_string()
            );
            assert_eq!(bedrock_model.model, "claude-sonnet-4");
            let Version::Date { date, .. } = &bedrock_model.version else {
                panic!("Expected date version");
            };
            let expected_dt: chrono::DateTime<chrono::Utc> =
                "2025-05-14T00:00:00Z".parse().unwrap();
            assert_eq!(*date, expected_dt);
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Bedrock ModelId with Anthropic provider");
        }
    }

    #[test]
    fn test_ollama_gemma3_basic_model() {
        let model_id_str = "gemma3";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Ollama,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Ollama(ollama_model)) = &result {
            assert_eq!(ollama_model.model, "gemma3");
            assert_eq!(ollama_model.tag, None);

            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Ollama ModelId");
        }
    }

    #[test]
    fn test_ollama_llama32_basic_model() {
        let model_id_str = "llama3.2";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Ollama,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Ollama(ollama_model)) = &result {
            assert_eq!(ollama_model.model, "llama3.2");
            assert_eq!(ollama_model.tag, None);
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Ollama ModelId");
        }
    }

    #[test]
    fn test_ollama_phi4_mini_basic_model() {
        let model_id_str = "phi4-mini";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Ollama,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Ollama(ollama_model)) = &result {
            assert_eq!(ollama_model.model, "phi4-mini");
            assert_eq!(ollama_model.tag, None);
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Ollama ModelId");
        }
    }

    #[test]
    fn test_ollama_llama32_vision_basic_model() {
        let model_id_str = "llama3.2-vision";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Ollama,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Ollama(ollama_model)) = &result {
            assert_eq!(ollama_model.model, "llama3.2-vision");
            assert_eq!(ollama_model.tag, None);
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Ollama ModelId");
        }
    }

    #[test]
    fn test_ollama_deepseek_r1_basic_model() {
        let model_id_str = "deepseek-r1";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Ollama,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Ollama(ollama_model)) = &result {
            assert_eq!(ollama_model.model, "deepseek-r1");
            assert_eq!(ollama_model.tag, None);
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Ollama ModelId");
        }
    }

    #[test]
    fn test_ollama_gemma3_1b_tagged_model() {
        let model_id_str = "gemma3:1b";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Ollama,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Ollama(ollama_model)) = &result {
            assert_eq!(ollama_model.model, "gemma3");
            assert_eq!(ollama_model.tag, Some("1b".to_string()));

            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Ollama ModelId with tag");
        }
    }

    #[test]
    fn test_ollama_gemma3_12b_tagged_model() {
        let model_id_str = "gemma3:12b";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Ollama,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Ollama(ollama_model)) = &result {
            assert_eq!(ollama_model.model, "gemma3");
            assert_eq!(ollama_model.tag, Some("12b".to_string()));
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Ollama ModelId with tag");
        }
    }

    #[test]
    fn test_ollama_deepseek_r1_671b_tagged_model() {
        let model_id_str = "deepseek-r1:671b";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Ollama,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Ollama(ollama_model)) = &result {
            assert_eq!(ollama_model.model, "deepseek-r1");
            assert_eq!(ollama_model.tag, Some("671b".to_string()));
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Ollama ModelId with tag");
        }
    }

    #[test]
    fn test_ollama_llama4_scout_tagged_model() {
        let model_id_str = "llama4:scout";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Ollama,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Ollama(ollama_model)) = &result {
            assert_eq!(ollama_model.model, "llama4");
            assert_eq!(ollama_model.tag, Some("scout".to_string()));
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Ollama ModelId with tag");
        }
    }

    #[test]
    fn test_ollama_llama4_maverick_tagged_model() {
        let model_id_str = "llama4:maverick";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Ollama,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Ollama(ollama_model)) = &result {
            assert_eq!(ollama_model.model, "llama4");
            assert_eq!(ollama_model.tag, Some("maverick".to_string()));
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Ollama ModelId with tag");
        }
    }

    #[test]
    fn test_ollama_llama_2_uncensored_freeform() {
        let model_id_str = "Llama 2 Uncensored";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Ollama,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Ollama(ollama_model)) = &result {
            assert_eq!(ollama_model.model, "Llama 2 Uncensored");
            assert_eq!(ollama_model.tag, None);
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Ollama ModelId");
        }
    }

    #[test]
    fn test_bedrock_with_geo_field() {
        let model_id_str = "us.anthropic.claude-3-sonnet-20240229-v1:0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.geo, Some("us".to_string()));
            assert_eq!(
                bedrock_model.provider,
                InferenceProvider::Anthropic.to_string()
            );
            assert_eq!(bedrock_model.model, "claude-3-sonnet");
            let Version::Date { date, .. } = &bedrock_model.version else {
                panic!("Expected date version");
            };
            let expected_dt: chrono::DateTime<chrono::Utc> =
                "2024-02-29T00:00:00Z".parse().unwrap();
            assert_eq!(*date, expected_dt);
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Bedrock ModelId with geo field");
        }
    }

    #[test]
    fn test_bedrock_with_geo_field_no_version() {
        let model_id_str = "eu.amazon.titan-embed-text-v1:0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.geo, Some("eu".to_string()));
            assert_eq!(bedrock_model.provider, "amazon");
            assert_eq!(bedrock_model.model, "titan-embed-text");
            assert!(matches!(bedrock_model.version, Version::ImplicitLatest));
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Bedrock ModelId with geo field");
        }
    }

    #[test]
    fn test_invalid_bedrock_unknown_provider_model() {
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            "some-unknown-provider.model",
        );

        assert!(result.is_err());
        if let Err(MapperError::InvalidModelName(provider)) = result {
            assert_eq!(provider, "some-unknown-provider.model");
        } else {
            panic!("Expected ProviderNotSupported error for unknown provider");
        }
    }

    #[test]
    fn test_invalid_bedrock_no_dot_separator() {
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            "custom-local-model",
        );
        assert!(result.is_err());
        if let Err(MapperError::InvalidModelName(model_name)) = result {
            assert_eq!(model_name, "custom-local-model");
        } else {
            panic!(
                "Expected InvalidModelName error for model without dot \
                 separator"
            );
        }
    }

    #[test]
    fn test_invalid_bedrock_malformed_format() {
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            "experimental@format#unknown",
        );
        assert!(result.is_err());
        if let Err(MapperError::InvalidModelName(model_name)) = result {
            assert_eq!(model_name, "experimental@format#unknown");
        } else {
            panic!("Expected InvalidModelName error for malformed format");
        }
    }

    #[test]
    fn test_edge_case_empty_string() {
        let result =
            ModelId::from_str_and_provider(InferenceProvider::OpenAI, "");
        assert!(result.is_err());
        if let Err(MapperError::InvalidModelName(msg)) = result {
            assert_eq!(msg, "Model name cannot be empty");
        } else {
            panic!("Expected InvalidModelName error for empty string");
        }
    }

    #[test]
    fn test_edge_case_single_char() {
        let model_id_str = "a";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::OpenAI,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        }) = &result
        {
            assert!(matches!(provider, InferenceProvider::OpenAI));
            assert_eq!(model_with_version.model, "a");
            assert!(matches!(
                model_with_version.version,
                Version::ImplicitLatest
            ));

            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected OpenAI ModelId for single character");
        }
    }

    #[test]
    fn test_edge_case_trailing_dash() {
        let result =
            ModelId::from_str_and_provider(InferenceProvider::OpenAI, "model-");
        assert!(result.is_err());
        if let Err(MapperError::InvalidModelName(msg)) = result {
            assert_eq!(msg, "Model name cannot end with dash");
        } else {
            panic!("Expected InvalidModelName error for trailing dash");
        }
    }

    #[test]
    fn test_edge_case_at_symbol() {
        let result =
            ModelId::from_str_and_provider(InferenceProvider::OpenAI, "model@");
        assert!(result.is_err());
        if let Err(MapperError::InvalidModelName(msg)) = result {
            assert_eq!(msg, "Model name cannot end with @ symbol");
        } else {
            panic!("Expected InvalidModelName error for @ symbol");
        }
    }

    #[test]
    fn test_edge_case_trailing_dot() {
        let result = ModelId::from_str_and_provider(
            InferenceProvider::OpenAI,
            "provider.",
        );
        assert!(result.is_err());
        if let Err(MapperError::InvalidModelName(msg)) = result {
            assert_eq!(msg, "Model name cannot end with dot");
        } else {
            panic!("Expected InvalidModelName error for trailing dot");
        }
    }

    #[test]
    fn test_edge_case_at_only() {
        let result =
            ModelId::from_str_and_provider(InferenceProvider::OpenAI, "@");
        assert!(result.is_err());
        if let Err(MapperError::InvalidModelName(msg)) = result {
            assert_eq!(msg, "Model name cannot end with @ symbol");
        } else {
            panic!("Expected InvalidModelName error for @ only");
        }
    }

    #[test]
    fn test_edge_case_dash_only() {
        let result =
            ModelId::from_str_and_provider(InferenceProvider::OpenAI, "-");
        assert!(result.is_err());
        if let Err(MapperError::InvalidModelName(msg)) = result {
            assert_eq!(msg, "Model name cannot end with dash");
        } else {
            panic!("Expected InvalidModelName error for dash only");
        }
    }

    #[test]
    fn test_provider_specific_model_variants() {
        let openai_result =
            ModelId::from_str_and_provider(InferenceProvider::OpenAI, "gpt-4");
        assert!(matches!(
            openai_result,
            Ok(ModelId::ModelIdWithVersion {
                provider: InferenceProvider::OpenAI,
                ..
            })
        ));

        let anthropic_result = ModelId::from_str_and_provider(
            InferenceProvider::Anthropic,
            "claude-3-sonnet",
        );
        assert!(matches!(
            anthropic_result,
            Ok(ModelId::ModelIdWithVersion {
                provider: InferenceProvider::Anthropic,
                ..
            })
        ));

        let bedrock_result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            "anthropic.claude-3-sonnet-20240229-v1:0",
        );
        assert!(matches!(bedrock_result, Ok(ModelId::Bedrock(_))));

        let ollama_result =
            ModelId::from_str_and_provider(InferenceProvider::Ollama, "llama3");
        assert!(matches!(ollama_result, Ok(ModelId::Ollama(_))));
    }

    #[test]
    fn test_from_str_openai_model() {
        let model_str = "openai/gpt-4";
        let result = ModelId::from_str(model_str).unwrap();

        if let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = result
        {
            assert!(matches!(provider, InferenceProvider::OpenAI));
            assert_eq!(model_with_version.model, "gpt-4");
            assert!(matches!(
                model_with_version.version,
                Version::ImplicitLatest
            ));
        } else {
            panic!("Expected ModelIdWithVersion with OpenAI provider");
        }
    }

    #[test]
    fn test_from_str_anthropic_model() {
        let model_str = "anthropic/claude-3-sonnet-20240229";
        let result = ModelId::from_str(model_str).unwrap();

        if let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = result
        {
            assert!(matches!(provider, InferenceProvider::Anthropic));
            assert_eq!(model_with_version.model, "claude-3-sonnet");
            if let Version::Date { date, .. } = model_with_version.version {
                let expected_dt: DateTime<Utc> =
                    "2024-02-29T00:00:00Z".parse().unwrap();
                assert_eq!(date, expected_dt);
            } else {
                panic!("Expected date version");
            }
        } else {
            panic!("Expected ModelIdWithVersion with Anthropic provider");
        }
    }

    #[test]
    fn test_from_str_anthropic_claude_opus_4_0_model() {
        let model_str = "anthropic/claude-opus-4-0";
        let result = ModelId::from_str(model_str).unwrap();

        if let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = result
        {
            assert!(matches!(provider, InferenceProvider::Anthropic));
            assert_eq!(model_with_version.model, "claude-opus-4-0");
            assert!(matches!(
                model_with_version.version,
                Version::ImplicitLatest
            ));
        } else {
            panic!("Expected ModelIdWithVersion with Anthropic provider");
        }
    }

    #[test]
    fn test_from_str_anthropic_claude_sonnet_4_0_model() {
        let model_str = "anthropic/claude-sonnet-4-0";
        let result = ModelId::from_str(model_str).unwrap();

        if let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = result
        {
            assert!(matches!(provider, InferenceProvider::Anthropic));
            assert_eq!(model_with_version.model, "claude-sonnet-4-0");
            assert!(matches!(
                model_with_version.version,
                Version::ImplicitLatest
            ));
        } else {
            panic!("Expected ModelIdWithVersion with Anthropic provider");
        }
    }

    #[test]
    fn test_from_str_bedrock_model() {
        let model_str = "bedrock/anthropic.claude-3-sonnet-20240229-v1:0";
        let result = ModelId::from_str(model_str).unwrap();

        if let ModelId::Bedrock(bedrock_model) = result {
            assert_eq!(
                bedrock_model.provider,
                InferenceProvider::Anthropic.to_string()
            );
            assert_eq!(bedrock_model.model, "claude-3-sonnet");
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");
        } else {
            panic!("Expected Bedrock ModelId");
        }
    }

    #[test]
    fn test_from_str_ollama_model() {
        let model_str = "ollama/llama3:8b";
        let result = ModelId::from_str(model_str).unwrap();

        if let ModelId::Ollama(ollama_model) = result {
            assert_eq!(ollama_model.model, "llama3");
            assert_eq!(ollama_model.tag, Some("8b".to_string()));
        } else {
            panic!("Expected Ollama ModelId");
        }
    }

    #[test]
    fn test_from_str_google_gemini_model() {
        let model_str = "gemini/gemini-pro";
        let result = ModelId::from_str(model_str).unwrap();

        if let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = result
        {
            assert!(matches!(provider, InferenceProvider::GoogleGemini));
            assert_eq!(model_with_version.model, "gemini-pro");
            assert!(matches!(
                model_with_version.version,
                Version::ImplicitLatest
            ));
        } else {
            panic!("Expected ModelIdWithVersion with GoogleGemini provider");
        }
    }

    #[test]
    fn test_from_str_invalid_no_slash() {
        let result = ModelId::from_str("gpt-4");
        assert!(result.is_err());
        if let Err(MapperError::InvalidModelName(msg)) = result {
            assert_eq!(
                msg,
                "Model string must be in format '{provider}/{model_name}', \
                 got 'gpt-4'"
            );
        } else {
            panic!("Expected InvalidModelName error");
        }
    }

    #[test]
    fn test_from_str_invalid_empty_model() {
        let result = ModelId::from_str("openai/");
        assert!(result.is_err());
        if let Err(MapperError::InvalidModelName(msg)) = result {
            assert_eq!(msg, "Model name cannot be empty after provider");
        } else {
            panic!("Expected InvalidModelName error");
        }
    }

    #[test]
    fn test_version_implicit_latest_from_empty_string() {
        let version = Version::from_str("").unwrap();
        assert!(matches!(version, Version::ImplicitLatest));
    }

    #[test]
    fn test_version_implicit_latest_display() {
        let version = Version::ImplicitLatest;
        assert_eq!(version.to_string(), "");
    }

    #[test]
    fn test_version_implicit_latest_serialization() {
        let version = Version::ImplicitLatest;
        let serialized = serde_json::to_string(&version).unwrap();
        assert_eq!(serialized, "\"\"");
    }

    #[test]
    fn test_version_implicit_latest_deserialization() {
        let json = "\"\"";
        let version: Version = serde_json::from_str(json).unwrap();
        assert!(matches!(version, Version::ImplicitLatest));
    }

    #[test]
    fn test_version_implicit_latest_roundtrip() {
        let original = Version::ImplicitLatest;
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: Version = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_bedrock_mistral_models() {
        // Test Mistral 7B model
        let model_id_str = "mistral.mistral-7b-instruct-v0:2";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.provider, "mistral");
            assert_eq!(bedrock_model.model, "mistral-7b-instruct");
            assert!(matches!(bedrock_model.version, Version::ImplicitLatest));
            assert_eq!(bedrock_model.bedrock_internal_version, "v0:2");
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Bedrock ModelId with Mistral provider");
        }

        // Test Mistral Large model
        let model_id_str = "mistral.mistral-large-2402-v1:0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.provider, "mistral");
            assert_eq!(bedrock_model.model, "mistral-large-2402");
            assert!(matches!(bedrock_model.version, Version::ImplicitLatest));
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");
        } else {
            panic!("Expected Bedrock ModelId with Mistral provider");
        }
    }

    #[test]
    fn test_bedrock_cohere_models() {
        // Test Cohere Command model
        let model_id_str = "cohere.command-text-v14";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.provider, "cohere");
            assert_eq!(bedrock_model.model, "command-text");
            assert!(matches!(bedrock_model.version, Version::ImplicitLatest));
            assert_eq!(bedrock_model.bedrock_internal_version, "v14");
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Bedrock ModelId with Cohere provider");
        }

        // Test Cohere Command R model
        let model_id_str = "cohere.command-r-v1:0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.provider, "cohere");
            assert_eq!(bedrock_model.model, "command-r");
            assert!(matches!(bedrock_model.version, Version::ImplicitLatest));
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Bedrock ModelId with Cohere provider");
        }
    }

    #[test]
    fn test_bedrock_stability_models() {
        // Test Stability AI SDXL model
        let model_id_str = "stability.stable-diffusion-xl-v1";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.provider, "stability");
            assert_eq!(bedrock_model.model, "stable-diffusion-xl");
            assert!(matches!(bedrock_model.version, Version::ImplicitLatest));
            assert_eq!(bedrock_model.bedrock_internal_version, "v1");
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Bedrock ModelId with Stability provider");
        }
    }

    #[test]
    fn test_bedrock_amazon_nova_models() {
        // Test Amazon Nova Pro model
        let model_id_str = "amazon.nova-pro-v1:0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.provider, "amazon");
            assert_eq!(bedrock_model.model, "nova-pro");
            assert!(matches!(bedrock_model.version, Version::ImplicitLatest));
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Bedrock ModelId with Amazon provider");
        }

        // Test Amazon Nova Lite model
        let model_id_str = "amazon.nova-lite-v1:0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.provider, "amazon");
            assert_eq!(bedrock_model.model, "nova-lite");
            assert!(matches!(bedrock_model.version, Version::ImplicitLatest));
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");
        } else {
            panic!("Expected Bedrock ModelId with Amazon provider");
        }
    }

    #[test]
    fn test_bedrock_meta_llama3_models() {
        // Test Llama 3.1 70B model
        let model_id_str = "meta.llama3-1-70b-instruct-v1:0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.provider, "meta");
            assert_eq!(bedrock_model.model, "llama3-1-70b-instruct");
            assert!(matches!(bedrock_model.version, Version::ImplicitLatest));
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Bedrock ModelId with Meta provider");
        }

        // Test Llama 3.1 405B model
        let model_id_str = "meta.llama3-1-405b-instruct-v1:0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.provider, "meta");
            assert_eq!(bedrock_model.model, "llama3-1-405b-instruct");
            assert!(matches!(bedrock_model.version, Version::ImplicitLatest));
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");
        } else {
            panic!("Expected Bedrock ModelId with Meta provider");
        }
    }

    #[test]
    fn test_bedrock_edge_cases() {
        // Test model with multiple dots in the name (will be parsed as
        // geo.provider.model)
        let model_id_str = "provider.model.name.with.dots-v1:0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.geo, Some("provider".to_string()));
            assert_eq!(bedrock_model.provider, "model");
            assert_eq!(bedrock_model.model, "name.with.dots");
            assert!(matches!(bedrock_model.version, Version::ImplicitLatest));
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");
        } else {
            panic!("Expected Bedrock ModelId");
        }

        // Test model with numbers in version
        let model_id_str = "provider.model-v2:1";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.provider, "provider");
            assert_eq!(bedrock_model.model, "model");
            assert!(matches!(bedrock_model.version, Version::ImplicitLatest));
            assert_eq!(bedrock_model.bedrock_internal_version, "v2:1");
        } else {
            panic!("Expected Bedrock ModelId");
        }

        // Test model with hyphenated provider name
        let model_id_str = "provider-name.model-v1:0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.provider, "provider-name");
            assert_eq!(bedrock_model.model, "model");
            assert!(matches!(bedrock_model.version, Version::ImplicitLatest));
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");
        } else {
            panic!("Expected Bedrock ModelId");
        }
    }

    #[test]
    fn test_bedrock_invalid_cases() {
        // Test missing version suffix
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            "provider.model",
        );
        assert!(result.is_err());
        if let Err(MapperError::InvalidModelName(model_name)) = result {
            assert_eq!(model_name, "provider.model");
        } else {
            panic!("Expected InvalidModelName error for missing version");
        }

        // Test model with version but no colon (actually valid for some
        // providers)
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            "provider.model-v1",
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.provider, "provider");
            assert_eq!(bedrock_model.model, "model");
            assert!(matches!(bedrock_model.version, Version::ImplicitLatest));
            assert_eq!(bedrock_model.bedrock_internal_version, "v1");
        } else {
            panic!("Expected Bedrock ModelId");
        }

        // Test empty provider (will actually parse with empty string provider)
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            ".model-v1:0",
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.provider, "");
            assert_eq!(bedrock_model.model, "model");
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");
        } else {
            panic!("Expected Bedrock ModelId with empty provider");
        }

        // Test model starting with dash (will parse dash as part of model name)
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            "provider.-model-v1:0",
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.provider, "provider");
            assert_eq!(bedrock_model.model, "-model");
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");
        } else {
            panic!("Expected Bedrock ModelId");
        }
    }

    #[test]
    fn test_bedrock_geo_with_various_providers() {
        // Test geo with Mistral
        let model_id_str = "eu-west-1.mistral.mistral-large-2402-v1:0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.geo, Some("eu-west-1".to_string()));
            assert_eq!(bedrock_model.provider, "mistral");
            assert_eq!(bedrock_model.model, "mistral-large-2402");
            assert!(matches!(bedrock_model.version, Version::ImplicitLatest));
            assert_eq!(bedrock_model.bedrock_internal_version, "v1:0");
            assert_eq!(result.as_ref().unwrap().to_string(), model_id_str);
        } else {
            panic!("Expected Bedrock ModelId with geo field");
        }

        // Test geo with Cohere
        let model_id_str = "ap-southeast-1.cohere.command-r-v1:0";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::Bedrock,
            model_id_str,
        );
        assert!(result.is_ok());
        if let Ok(ModelId::Bedrock(bedrock_model)) = &result {
            assert_eq!(bedrock_model.geo, Some("ap-southeast-1".to_string()));
            assert_eq!(bedrock_model.provider, "cohere");
            assert_eq!(bedrock_model.model, "command-r");
        } else {
            panic!("Expected Bedrock ModelId with geo field");
        }
    }

    #[test]
    fn test_parse_date_mmdd_format() {
        // Test MMDD format parsing
        let current_year = chrono::Utc::now().year();

        // Test a valid MMDD date
        let test_date = "0315"; // March 15
        if let Some((parsed_date, format)) = parse_date(test_date) {
            assert_eq!(format, "%m%d");
            assert_eq!(parsed_date.year(), current_year);
            assert_eq!(parsed_date.month(), 3);
            assert_eq!(parsed_date.day(), 15);
        } else {
            panic!("Failed to parse MMDD date");
        }

        // Test another MMDD date
        let test_date = "1225"; // December 25
        if let Some((parsed_date, format)) = parse_date(test_date) {
            assert_eq!(format, "%m%d");
            assert_eq!(parsed_date.year(), current_year);
            assert_eq!(parsed_date.month(), 12);
            assert_eq!(parsed_date.day(), 25);
        } else {
            panic!("Failed to parse MMDD date");
        }
    }

    #[test]
    fn test_model_with_mmdd_date_version() {
        // Test a model with MMDD date version
        let model_id_str = "gpt-4-0125";
        let result = ModelId::from_str_and_provider(
            InferenceProvider::OpenAI,
            model_id_str,
        );

        assert!(result.is_ok());
        let ModelId::ModelIdWithVersion {
            provider,
            id: model_with_version,
        } = result.unwrap()
        else {
            panic!("Expected ModelIdWithVersion");
        };

        assert!(matches!(provider, InferenceProvider::OpenAI));
        assert_eq!(model_with_version.model, "gpt-4");

        let Version::Date { date, format } = &model_with_version.version else {
            panic!("Expected date version");
        };

        assert_eq!(format, &"%m%d");
        assert_eq!(date.year(), chrono::Utc::now().year());
        assert_eq!(date.month(), 1);
        assert_eq!(date.day(), 25);

        assert_eq!(model_with_version.to_string(), model_id_str);
    }
}
