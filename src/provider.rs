use std::collections::BTreeSet;
use std::fmt;
use std::time::Duration;

use github_copilot_sdk::types::{NamedProviderConfig, ProviderModelConfig};
use serde::Deserialize;

pub const DEFAULT_PROVIDER_NAME: &str = "local";
pub const DEFAULT_PROVIDER_WIRE_API: &str = "completions";
pub const DEFAULT_PROVIDER_URL: &str = "http://localhost:11434/v1";
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderSettings {
    pub name: String,
    pub base_url: String,
    pub wire_api: String,
    pub api_key: Option<String>,
}

impl fmt::Debug for ProviderSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderSettings")
            .field("name", &self.name)
            .field("base_url", &self.base_url)
            .field("wire_api", &self.wire_api)
            .field("api_key", &self.api_key.as_ref().map(|_| "<set>"))
            .finish()
    }
}

impl ProviderSettings {
    pub fn new(
        name: impl Into<String>,
        base_url: impl AsRef<str>,
        wire_api: impl Into<String>,
        api_key: Option<String>,
    ) -> Result<Self, ProviderError> {
        let name = name.into();
        validate_provider_name(&name)?;
        let base_url = normalize_base_url(base_url.as_ref())?;
        let wire_api = wire_api.into();
        validate_wire_api(&wire_api)?;
        let api_key = api_key.filter(|api_key| !api_key.trim().is_empty());

        Ok(Self {
            name,
            base_url,
            wire_api,
            api_key,
        })
    }

    pub fn default_for(base_url: impl AsRef<str>) -> Result<Self, ProviderError> {
        Self::new(
            DEFAULT_PROVIDER_NAME,
            base_url,
            DEFAULT_PROVIDER_WIRE_API,
            None,
        )
    }
}

#[derive(Clone)]
pub struct ProviderRegistry {
    providers: Vec<NamedProviderConfig>,
    models: Vec<ProviderModelConfig>,
}

impl fmt::Debug for ProviderRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let provider_names: Vec<&str> = self
            .providers
            .iter()
            .map(|provider| provider.name.as_str())
            .collect();
        formatter
            .debug_struct("ProviderRegistry")
            .field("provider_names", &provider_names)
            .field("models", &self.models)
            .finish()
    }
}

impl ProviderRegistry {
    pub fn from_model_ids<I, S>(
        settings: &ProviderSettings,
        model_ids: I,
    ) -> Result<Self, ProviderError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let model_ids = normalize_model_ids(model_ids)?;
        if model_ids.is_empty() {
            return Err(ProviderError::NoModels);
        }

        let provider = NamedProviderConfig::new(settings.name.clone(), settings.base_url.clone())
            .with_provider_type("openai")
            .with_wire_api(settings.wire_api.clone());
        let provider = match settings.api_key.as_deref() {
            Some(api_key) => provider.with_bearer_token(api_key),
            None => provider,
        };
        let models = model_ids
            .into_iter()
            .map(|model_id| ProviderModelConfig::new(model_id, settings.name.clone()))
            .collect();

        Ok(Self {
            providers: vec![provider],
            models,
        })
    }

    pub fn providers(&self) -> &[NamedProviderConfig] {
        &self.providers
    }

    pub fn models(&self) -> &[ProviderModelConfig] {
        &self.models
    }

    pub fn qualified_model_ids(&self) -> Vec<String> {
        self.models
            .iter()
            .map(|model| format!("{}/{}", model.provider, model.id))
            .collect()
    }
}

#[derive(Debug)]
pub enum ProviderError {
    InvalidProviderName,
    InvalidBaseUrl,
    UnsupportedWireApi,
    Request(reqwest::Error),
    UnsuccessfulStatus(u16),
    MalformedResponse(serde_json::Error),
    EmptyModelId { index: usize },
    NoModels,
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProviderName => {
                write!(formatter, "provider name must be non-empty and must not contain '/'")
            }
            Self::InvalidBaseUrl => write!(
                formatter,
                "provider URL must be an absolute HTTP(S) URL without credentials, query, or fragment"
            ),
            Self::UnsupportedWireApi => {
                write!(formatter, "provider wire API must be 'completions' or 'responses'")
            }
            Self::Request(_) => write!(formatter, "could not query the provider model catalog"),
            Self::UnsuccessfulStatus(status) => {
                write!(formatter, "provider model catalog returned HTTP status {status}")
            }
            Self::MalformedResponse(_) => {
                write!(formatter, "provider model catalog returned invalid JSON")
            }
            Self::EmptyModelId { index } => write!(
                formatter,
                "provider model catalog entry {index} has an empty model id"
            ),
            Self::NoModels => write!(formatter, "provider model catalog contains no models"),
        }
    }
}

impl std::error::Error for ProviderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Request(error) => Some(error),
            Self::MalformedResponse(error) => Some(error),
            Self::InvalidProviderName
            | Self::InvalidBaseUrl
            | Self::UnsupportedWireApi
            | Self::UnsuccessfulStatus(_)
            | Self::EmptyModelId { .. }
            | Self::NoModels => None,
        }
    }
}

pub fn validate_provider_name(name: &str) -> Result<(), ProviderError> {
    if name.trim().is_empty() || name.contains('/') {
        return Err(ProviderError::InvalidProviderName);
    }
    Ok(())
}

pub fn validate_wire_api(wire_api: &str) -> Result<(), ProviderError> {
    if matches!(wire_api, "completions" | "responses") {
        Ok(())
    } else {
        Err(ProviderError::UnsupportedWireApi)
    }
}

pub fn normalize_base_url(base_url: &str) -> Result<String, ProviderError> {
    let parsed = reqwest::Url::parse(base_url.trim()).map_err(|_| ProviderError::InvalidBaseUrl)?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(ProviderError::InvalidBaseUrl);
    }

    let mut normalized = parsed.to_string();
    while normalized.ends_with('/') {
        normalized.pop();
    }
    Ok(normalized)
}

pub fn normalize_model_ids<I, S>(model_ids: I) -> Result<Vec<String>, ProviderError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut normalized = BTreeSet::new();
    for (index, model_id) in model_ids.into_iter().enumerate() {
        let model_id = model_id.as_ref().trim();
        if model_id.is_empty() {
            return Err(ProviderError::EmptyModelId { index });
        }
        normalized.insert(model_id.to_string());
    }
    Ok(normalized.into_iter().collect())
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
}

pub fn parse_model_catalog(body: &str) -> Result<Vec<String>, ProviderError> {
    let response: ModelsResponse =
        serde_json::from_str(body).map_err(ProviderError::MalformedResponse)?;
    normalize_model_ids(response.data.into_iter().map(|model| model.id))
}

pub async fn discover(settings: &ProviderSettings) -> Result<ProviderRegistry, ProviderError> {
    validate_provider_name(&settings.name)?;
    validate_wire_api(&settings.wire_api)?;
    let base_url = normalize_base_url(&settings.base_url)?;
    let client = reqwest::Client::builder()
        .timeout(DISCOVERY_TIMEOUT)
        .build()
        .map_err(ProviderError::Request)?;
    let models_url = format!("{base_url}/models");
    let request = client.get(models_url);
    let request = match settings.api_key.as_deref() {
        Some(api_key) => request.bearer_auth(api_key),
        None => request,
    };
    let response = request.send().await.map_err(ProviderError::Request)?;
    let status = response.status();
    if !status.is_success() {
        return Err(ProviderError::UnsuccessfulStatus(status.as_u16()));
    }
    let body = response.text().await.map_err(ProviderError::Request)?;
    let model_ids = parse_model_catalog(&body)?;
    ProviderRegistry::from_model_ids(settings, model_ids)
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread::{self, JoinHandle};

    use super::{
        discover, normalize_base_url, normalize_model_ids, parse_model_catalog,
        validate_provider_name, ProviderError, ProviderRegistry, ProviderSettings,
        DEFAULT_PROVIDER_NAME, DEFAULT_PROVIDER_WIRE_API,
    };

    fn mock_models_server(status: u16, body: &str) -> (String, Arc<Mutex<String>>, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("mock server should bind");
        let address = listener
            .local_addr()
            .expect("mock server address should be available");
        let request = Arc::new(Mutex::new(String::new()));
        let captured_request = Arc::clone(&request);
        let body = body.to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("mock server should accept");
            let mut buffer = [0_u8; 4096];
            let bytes_read = stream
                .read(&mut buffer)
                .expect("mock request should be readable");
            *captured_request
                .lock()
                .expect("request lock should be available") =
                String::from_utf8_lossy(&buffer[..bytes_read]).into_owned();
            let response = format!(
                "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/json\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("mock response should be writable");
        });
        (format!("http://{address}"), request, handle)
    }

    #[test]
    fn normalizes_provider_urls_without_changing_the_path() {
        assert_eq!(
            normalize_base_url(" http://localhost:11434/v1/// ").unwrap(),
            "http://localhost:11434/v1"
        );
    }

    #[test]
    fn rejects_provider_urls_that_could_hide_credentials() {
        assert!(matches!(
            normalize_base_url("http://user:password@example.test/v1"),
            Err(ProviderError::InvalidBaseUrl)
        ));
        assert!(matches!(
            normalize_base_url("http://example.test/v1?api_key=secret"),
            Err(ProviderError::InvalidBaseUrl)
        ));
    }

    #[test]
    fn rejects_provider_names_that_break_qualified_model_ids() {
        assert!(validate_provider_name("local/provider").is_err());
        assert!(validate_provider_name(" ").is_err());
    }

    #[test]
    fn parses_and_sorts_unique_model_ids() {
        let ids =
            parse_model_catalog(r#"{"data":[{"id":"qwen:7b"},{"id":" llama "},{"id":"qwen:7b"}]}"#)
                .unwrap();

        assert_eq!(ids, vec!["llama", "qwen:7b"]);
    }

    #[test]
    fn preserves_model_ids_containing_slashes() {
        let ids = normalize_model_ids(["org/model:7b", "model"] as [&str; 2]).unwrap();
        assert_eq!(ids, vec!["model", "org/model:7b"]);
    }

    #[test]
    fn rejects_empty_or_malformed_catalogs() {
        assert!(matches!(
            parse_model_catalog(r#"{"data":[{"id":""}]}"#),
            Err(ProviderError::EmptyModelId { index: 0 })
        ));
        assert!(matches!(
            parse_model_catalog(r#"{"data":[]}"#),
            Ok(ids) if ids.is_empty()
        ));
        assert!(parse_model_catalog("not-json").is_err());
    }

    #[test]
    fn builds_an_additive_registry_with_bearer_auth() {
        let settings = ProviderSettings::new(
            DEFAULT_PROVIDER_NAME,
            "http://localhost:11434/v1/",
            DEFAULT_PROVIDER_WIRE_API,
            Some("secret".to_string()),
        )
        .unwrap();
        let registry = ProviderRegistry::from_model_ids(&settings, ["qwen:7b", "llama"]).unwrap();

        assert_eq!(registry.providers().len(), 1);
        assert_eq!(registry.providers()[0].name, "local");
        assert_eq!(
            registry.providers()[0].provider_type.as_deref(),
            Some("openai")
        );
        assert_eq!(
            registry.providers()[0].wire_api.as_deref(),
            Some("completions")
        );
        assert_eq!(
            registry.providers()[0].bearer_token.as_deref(),
            Some("secret")
        );
        assert_eq!(
            registry.qualified_model_ids(),
            vec!["local/llama", "local/qwen:7b"]
        );
        assert!(registry.models().iter().all(|model| {
            model.max_prompt_tokens.is_none()
                && model.max_context_window_tokens.is_none()
                && model.max_output_tokens.is_none()
                && model.capabilities.is_none()
        }));
    }

    #[test]
    fn rejects_an_empty_registry() {
        let settings = ProviderSettings::default_for("http://localhost:11434/v1").unwrap();
        assert!(matches!(
            ProviderRegistry::from_model_ids(&settings, std::iter::empty::<&str>()),
            Err(ProviderError::NoModels)
        ));
    }

    #[tokio::test]
    async fn discovers_models_and_sends_bearer_auth() {
        let (base_url, request, server) =
            mock_models_server(200, r#"{"data":[{"id":"qwen:7b"},{"id":"llama"}]}"#);
        let settings = ProviderSettings::new(
            "local",
            base_url,
            "completions",
            Some("secret-token".to_string()),
        )
        .unwrap();

        let registry = discover(&settings).await.expect("mock catalog should load");
        server.join().expect("mock server should finish");

        assert_eq!(
            registry.qualified_model_ids(),
            vec!["local/llama", "local/qwen:7b"]
        );
        assert!(request
            .lock()
            .unwrap()
            .to_ascii_lowercase()
            .contains("authorization: bearer secret-token"));
    }

    #[tokio::test]
    async fn reports_non_success_status_without_returning_response_details() {
        let (base_url, _, server) = mock_models_server(401, "unauthorized");
        let settings = ProviderSettings::new(
            "local",
            base_url,
            "completions",
            Some("secret-token".to_string()),
        )
        .unwrap();

        let error = discover(&settings)
            .await
            .expect_err("unauthorized catalog should fail");
        server.join().expect("mock server should finish");

        assert!(matches!(error, ProviderError::UnsuccessfulStatus(401)));
        assert!(!error.to_string().contains("secret-token"));
    }

    #[test]
    fn redacts_provider_credentials_from_debug_output() {
        let settings = ProviderSettings::new(
            "local",
            "http://localhost:11434/v1",
            "completions",
            Some("secret-token".to_string()),
        )
        .unwrap();
        let registry = ProviderRegistry::from_model_ids(&settings, ["qwen:7b"]).unwrap();

        let debug = format!("{registry:?}");
        assert!(!debug.contains("secret-token"));
        assert!(debug.contains("local"));
    }

    #[tokio::test]
    async fn rejects_malformed_and_empty_model_catalogs() {
        let (base_url, _, malformed_server) = mock_models_server(200, "not-json");
        let malformed_settings = ProviderSettings::default_for(base_url).unwrap();
        let malformed_error = discover(&malformed_settings)
            .await
            .expect_err("malformed catalog should fail");
        malformed_server
            .join()
            .expect("malformed mock server should finish");
        assert!(matches!(
            malformed_error,
            ProviderError::MalformedResponse(_)
        ));

        let (base_url, _, empty_server) = mock_models_server(200, r#"{"data":[]}"#);
        let empty_settings = ProviderSettings::default_for(base_url).unwrap();
        let empty_error = discover(&empty_settings)
            .await
            .expect_err("empty catalog should fail");
        empty_server
            .join()
            .expect("empty mock server should finish");
        assert!(matches!(empty_error, ProviderError::NoModels));
    }

    #[tokio::test]
    async fn rejects_catalog_entries_without_model_ids() {
        let (base_url, _, server) = mock_models_server(200, r#"{"data":[{"name":"missing"}]}"#);
        let settings = ProviderSettings::default_for(base_url).unwrap();

        let error = discover(&settings)
            .await
            .expect_err("catalog entries need model ids");
        server.join().expect("mock server should finish");

        assert!(matches!(error, ProviderError::MalformedResponse(_)));
    }
}
