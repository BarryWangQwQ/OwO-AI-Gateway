use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use owo_config::AuthMode;
use owo_core::{ErrorKind, ModelError};
use owo_routing::ProviderAccess;
use url::Url;

/// Joins a resource path onto the provider base URL (`https://x/v1` + `models` → `https://x/v1/models`)
/// and applies configured query parameters, including query-style auth.
pub fn endpoint(access: &ProviderAccess, resource: &str) -> Url {
    let p = &access.provider;
    let mut url = p.base_url.clone();
    let base = url.path().trim_end_matches('/').to_string();
    url.set_path(&format!("{base}/{resource}"));
    {
        let mut q = url.query_pairs_mut();
        for (k, v) in &p.query {
            q.append_pair(k, v);
        }
        if let (AuthMode::Query, Some(param), Some(secret)) = (p.auth, &p.auth_param, &access.secret) {
            q.append_pair(param, secret.expose());
        }
    }
    if url.query() == Some("") {
        url.set_query(None);
    }
    url
}

/// Configured static headers plus the credential, marked sensitive so it is never logged.
pub fn headers(access: &ProviderAccess) -> Result<HeaderMap, ModelError> {
    let p = &access.provider;
    let mut h = HeaderMap::new();
    let bad = |name: &str| {
        ModelError::new(ErrorKind::ConfigurationError, format!("provider `{}`: invalid header `{name}`", p.id))
    };
    for (k, v) in &p.headers {
        let name = HeaderName::from_bytes(k.as_bytes()).map_err(|_| bad(k))?;
        let value = HeaderValue::from_str(v).map_err(|_| bad(k))?;
        h.insert(name, value);
    }
    if let Some(secret) = &access.secret {
        let (name, value) = match p.auth {
            AuthMode::Bearer => (reqwest::header::AUTHORIZATION, format!("Bearer {}", secret.expose())),
            AuthMode::Header => {
                let param = p.auth_param.as_deref().unwrap_or("x-api-key");
                (HeaderName::from_bytes(param.as_bytes()).map_err(|_| bad(param))?, secret.expose().to_string())
            }
            AuthMode::Query | AuthMode::None => return Ok(h),
        };
        let mut value = HeaderValue::from_str(&value).map_err(|_| {
            ModelError::new(ErrorKind::ConfigurationError, format!("provider `{}`: credential is not a valid header value", p.id))
        })?;
        value.set_sensitive(true);
        h.insert(name, value);
    }
    Ok(h)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Arc;
    use owo_credentials::{CredentialRef, Secret};
    use owo_registry::Provider;

    pub(crate) fn access(base: &str, auth: AuthMode) -> ProviderAccess {
        let provider = Provider {
            id: "p".into(),
            display_name: "P".into(),
            adapter: "openai-chat".into(),
            base_url: Url::parse(base).unwrap(),
            auth,
            auth_param: Some("key".into()),
            api_key: CredentialRef::Env("X".into()),
            headers: [("X-Title".to_string(), "OwO AI Gateway".to_string())].into(),
            query: [("api-version".to_string(), "1".to_string())].into(),
            models: vec![],
            model_defaults: Default::default(),
            allow_direct_models: true,
            enabled: true,
            allow_private_network: false,
            preset: None,
        };
        ProviderAccess { provider: Arc::new(provider), secret: Some(Secret::new("sk-secret-value")) }
    }

    #[test]
    fn builds_endpoints() {
        let a = access("https://api.example.com/v1/", AuthMode::Bearer);
        assert_eq!(endpoint(&a, "chat/completions").as_str(), "https://api.example.com/v1/chat/completions?api-version=1");
        let a = access("https://api.deepseek.com", AuthMode::Query);
        assert_eq!(
            endpoint(&a, "models").as_str(),
            "https://api.deepseek.com/models?api-version=1&key=sk-secret-value"
        );
    }

    #[test]
    fn auth_headers_are_sensitive() {
        let h = headers(&access("https://x", AuthMode::Bearer)).unwrap();
        let auth = h.get("authorization").unwrap();
        assert!(auth.is_sensitive());
        assert_eq!(auth, "Bearer sk-secret-value");
        assert_eq!(h.get("x-title").unwrap(), "OwO AI Gateway");
        let h = headers(&access("https://x", AuthMode::Header)).unwrap();
        assert_eq!(h.get("key").unwrap(), "sk-secret-value");
    }
}
