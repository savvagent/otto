use otto_host::HttpAuth;
use rmcp::transport::auth::{
    AuthClient, AuthError, AuthorizationManager, AuthorizationMetadata, CredentialStore,
    OAuthClientConfig, OAuthTokenResponse, StoredCredentials,
};
use serde::{Deserialize, Serialize};

use crate::creds;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct StoredMcpOAuthSecret {
    pub issuer: String,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    pub redirect_uri: String,
    #[serde(default)]
    pub requested_scopes: Vec<String>,
    #[serde(default)]
    pub granted_scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_response: Option<OAuthTokenResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_received_at: Option<u64>,
}

impl std::fmt::Debug for StoredMcpOAuthSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredMcpOAuthSecret")
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field("redirect_uri", &self.redirect_uri)
            .field("requested_scopes", &self.requested_scopes)
            .field("granted_scopes", &self.granted_scopes)
            .field(
                "token_response",
                &self.token_response.as_ref().map(|_| "[REDACTED]"),
            )
            .field("token_received_at", &self.token_received_at)
            .finish()
    }
}

impl StoredMcpOAuthSecret {
    fn validate_for_startup(&self) -> Result<(), String> {
        if self.issuer.trim().is_empty() {
            return Err("stored oauth issuer is missing".into());
        }
        if self.client_id.trim().is_empty() {
            return Err("stored oauth client_id is missing".into());
        }
        if self.redirect_uri.trim().is_empty() {
            return Err("stored oauth redirect_uri is missing".into());
        }
        if self.token_response.is_none() {
            return Err("oauth authorization required; open /mcp to authorize".into());
        }
        Ok(())
    }

    fn to_stored_credentials(&self) -> StoredCredentials {
        StoredCredentials::new(
            self.client_id.clone(),
            self.token_response.clone(),
            self.granted_scopes.clone(),
            self.token_received_at,
        )
    }

    fn apply_stored_credentials(&mut self, credentials: StoredCredentials) {
        self.client_id = credentials.client_id;
        self.token_response = credentials.token_response;
        self.granted_scopes = credentials.granted_scopes;
        self.token_received_at = credentials.token_received_at;
    }
}

#[derive(Clone, Debug)]
pub(crate) struct KeyringOAuthCredentialStore {
    server_name: String,
    seed: StoredMcpOAuthSecret,
}

impl KeyringOAuthCredentialStore {
    pub(crate) fn new(server_name: impl Into<String>, seed: StoredMcpOAuthSecret) -> Self {
        Self {
            server_name: server_name.into(),
            seed,
        }
    }
}

#[async_trait::async_trait]
impl CredentialStore for KeyringOAuthCredentialStore {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        let secret =
            creds::mcp_load_json::<StoredMcpOAuthSecret>(&self.server_name).map_err(|err| {
                AuthError::InternalError(format!(
                    "failed to read oauth keyring state for `mcp:{}`: {err}",
                    self.server_name
                ))
            })?;
        Ok(secret.map(|secret| secret.to_stored_credentials()))
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        let mut secret = match creds::mcp_load_json::<StoredMcpOAuthSecret>(&self.server_name) {
            Ok(Some(existing)) => existing,
            Ok(None) => self.seed.clone(),
            Err(err) => {
                return Err(AuthError::InternalError(format!(
                    "failed to read oauth keyring state for `mcp:{}` before save: {err}",
                    self.server_name
                )));
            }
        };
        secret.apply_stored_credentials(credentials);
        creds::mcp_save_json(&self.server_name, &secret).map_err(|err| {
            AuthError::InternalError(format!(
                "failed to persist oauth keyring state for `mcp:{}`: {err}",
                self.server_name
            ))
        })
    }

    async fn clear(&self) -> Result<(), AuthError> {
        let mut secret = match creds::mcp_load_json::<StoredMcpOAuthSecret>(&self.server_name) {
            Ok(Some(existing)) => existing,
            Ok(None) => self.seed.clone(),
            Err(err) => {
                return Err(AuthError::InternalError(format!(
                    "failed to read oauth keyring state for `mcp:{}` before clear: {err}",
                    self.server_name
                )));
            }
        };
        secret.token_response = None;
        secret.token_received_at = None;
        creds::mcp_save_json(&self.server_name, &secret).map_err(|err| {
            AuthError::InternalError(format!(
                "failed to clear oauth keyring state for `mcp:{}`: {err}",
                self.server_name
            ))
        })
    }
}

pub(crate) fn load_stored_secret(
    server_name: &str,
) -> Result<Option<StoredMcpOAuthSecret>, String> {
    creds::mcp_load_json(server_name)
        .map_err(|err| format!("failed to read oauth keyring state for `mcp:{server_name}`: {err}"))
}

pub(crate) async fn build_startup_http_auth(
    server_name: &str,
    resource_url: &str,
    stored: StoredMcpOAuthSecret,
) -> Result<HttpAuth, String> {
    stored.validate_for_startup()?;

    let mut manager = AuthorizationManager::new(resource_url)
        .await
        .map_err(|err| format!("failed to prepare oauth manager: {err}"))?;
    let metadata = manager
        .discover_metadata()
        .await
        .map_err(|err| format!("oauth metadata discovery failed: {err}"))?;
    validate_discovered_metadata(&stored, &metadata)?;

    manager.set_metadata(metadata);
    manager.set_credential_store(KeyringOAuthCredentialStore::new(
        server_name,
        stored.clone(),
    ));

    let mut config = OAuthClientConfig::new(stored.client_id.clone(), stored.redirect_uri.clone())
        .with_scopes(stored.requested_scopes.clone());
    if let Some(secret) = stored.client_secret.clone() {
        config = config.with_client_secret(secret);
    }
    manager
        .configure_client(config)
        .map_err(|err| format!("oauth client configuration failed: {err}"))?;

    Ok(HttpAuth::OAuth {
        client: AuthClient::new(reqwest::Client::new(), manager),
    })
}

pub(crate) fn validate_discovered_metadata(
    stored: &StoredMcpOAuthSecret,
    metadata: &AuthorizationMetadata,
) -> Result<(), String> {
    match metadata.issuer.as_deref() {
        Some(issuer) if issuer == stored.issuer => {}
        Some(issuer) => {
            return Err(format!(
                "oauth issuer mismatch: expected `{}`, discovered `{issuer}`",
                stored.issuer
            ));
        }
        None => {
            return Err("authorization server metadata is missing issuer".into());
        }
    }

    if let Some(response_types) = metadata.response_types_supported.as_ref()
        && !response_types
            .iter()
            .any(|response_type| response_type == "code")
    {
        return Err(
            "authorization server metadata does not advertise authorization-code support".into(),
        );
    }

    match metadata.code_challenge_methods_supported.as_ref() {
        Some(methods) if methods.iter().any(|method| method == "S256") => Ok(()),
        Some(methods) => Err(format!(
            "authorization server metadata does not advertise PKCE S256 support: {methods:?}"
        )),
        None => Err(
            "authorization server metadata is missing code_challenge_methods_supported; refusing OAuth without explicit PKCE S256 support".into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        http::{HeaderValue, StatusCode, header::CONTENT_TYPE},
        routing::get,
    };
    use serde_json::json;
    use std::net::SocketAddr;
    use tokio::net::TcpListener;

    fn make_token_response(access_token: &str, expires_in_secs: Option<u64>) -> OAuthTokenResponse {
        let mut body = json!({
            "access_token": access_token,
            "token_type": "Bearer",
        });
        if let Some(secs) = expires_in_secs {
            body["expires_in"] = json!(secs);
        }
        serde_json::from_value(body).expect("token response json")
    }

    fn stored_secret(issuer: &str) -> StoredMcpOAuthSecret {
        StoredMcpOAuthSecret {
            issuer: issuer.to_string(),
            client_id: "otto-client".into(),
            client_secret: None,
            redirect_uri: "http://127.0.0.1:43111/oauth/callback".into(),
            requested_scopes: vec!["mcp.read".into()],
            granted_scopes: vec!["mcp.read".into()],
            token_response: Some(make_token_response("access-token", Some(3600))),
            token_received_at: Some(1),
        }
    }

    async fn spawn_metadata_server(
        issuer_suffix: &str,
        code_challenge_methods_supported: Option<Vec<&'static str>>,
    ) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let issuer = format!("http://127.0.0.1:{}/{}", addr.port(), issuer_suffix);
        let router = Router::new()
            .route(
                "/.well-known/oauth-protected-resource/mcp",
                get({
                    let issuer = issuer.clone();
                    move || async move {
                        (
                            StatusCode::OK,
                            [(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
                            serde_json::to_string(&json!({
                                "authorization_servers": [issuer],
                                "scopes_supported": ["mcp.read"],
                            }))
                            .unwrap(),
                        )
                    }
                }),
            )
            .route(
                &format!("/.well-known/oauth-authorization-server/{issuer_suffix}"),
                get({
                    let issuer = issuer.clone();
                    let methods = code_challenge_methods_supported.clone();
                    move || async move {
                        (
                            StatusCode::OK,
                            [(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
                            serde_json::to_string(&json!({
                                "issuer": issuer,
                                "authorization_endpoint": format!("{}/authorize", issuer),
                                "token_endpoint": format!("{}/token", issuer),
                                "response_types_supported": ["code"],
                                "code_challenge_methods_supported": methods,
                            }))
                            .unwrap(),
                        )
                    }
                }),
            );
        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (addr, handle)
    }

    #[tokio::test]
    async fn startup_builder_accepts_matching_metadata_with_s256() {
        let (addr, handle) = spawn_metadata_server("issuer", Some(vec!["S256"])).await;
        let auth = build_startup_http_auth(
            "remote",
            &format!("http://127.0.0.1:{}/mcp", addr.port()),
            stored_secret(&format!("http://127.0.0.1:{}/issuer", addr.port())),
        )
        .await;
        handle.abort();
        assert!(matches!(auth, Ok(HttpAuth::OAuth { .. })));
    }

    #[tokio::test]
    async fn startup_builder_rejects_issuer_mismatch() {
        let (addr, handle) = spawn_metadata_server("issuer", Some(vec!["S256"])).await;
        let err = build_startup_http_auth(
            "remote",
            &format!("http://127.0.0.1:{}/mcp", addr.port()),
            stored_secret("http://127.0.0.1:9/wrong"),
        )
        .await
        .unwrap_err();
        handle.abort();
        assert!(err.contains("issuer mismatch"), "err: {err}");
    }

    #[tokio::test]
    async fn startup_builder_rejects_missing_s256_support() {
        let (addr, handle) = spawn_metadata_server("issuer", Some(vec!["plain"])).await;
        let err = build_startup_http_auth(
            "remote",
            &format!("http://127.0.0.1:{}/mcp", addr.port()),
            stored_secret(&format!("http://127.0.0.1:{}/issuer", addr.port())),
        )
        .await
        .unwrap_err();
        handle.abort();
        assert!(err.contains("PKCE S256"), "err: {err}");
    }
}
