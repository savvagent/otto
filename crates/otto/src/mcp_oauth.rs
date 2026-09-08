use otto_host::HttpAuth;
use rmcp::transport::auth::{
    AuthClient, AuthError, AuthorizationManager, AuthorizationMetadata, CredentialStore,
    InMemoryStateStore, OAuthClientConfig, OAuthTokenResponse, StoredCredentials,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, oneshot};

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
    validate_protected_resource_url(resource_url)?;
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
    validate_authorization_server_url("authorization server issuer", &stored.issuer)?;
    validate_authorization_server_url("authorization endpoint", &metadata.authorization_endpoint)?;
    validate_authorization_server_url("token endpoint", &metadata.token_endpoint)?;
    if let Some(registration_endpoint) = metadata.registration_endpoint.as_deref() {
        validate_authorization_server_url("registration endpoint", registration_endpoint)?;
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

fn validate_authorization_server_url(label: &str, url: &str) -> Result<(), String> {
    let parsed =
        reqwest::Url::parse(url).map_err(|err| format!("{label} is not a valid URL: {err}"))?;
    if parsed.scheme() == "https" {
        return Ok(());
    }
    if parsed.scheme() == "http" && is_loopback_host(parsed.host_str()) {
        return Ok(());
    }
    Err(format!(
        "{label} must use https (or http loopback for local development): {url}"
    ))
}

fn validate_protected_resource_url(url: &str) -> Result<(), String> {
    validate_authorization_server_url("oauth-protected MCP resource URL", url)
}

fn is_loopback_host(host: Option<&str>) -> bool {
    matches!(host, Some("127.0.0.1" | "localhost" | "::1"))
}

fn authorization_response_iss_required(metadata: &AuthorizationMetadata) -> bool {
    metadata
        .additional_fields
        .get("authorization_response_iss_parameter_supported")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BeginAuthorizationResult {
    pub authorization_url: String,
    pub redirect_uri: String,
    pub reused_client_registration: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PollAuthorizationResult {
    NotStarted,
    Pending { redirect_uri: String },
    Completed { message: String },
    Failed { message: String },
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct OAuthCallbackParams {
    pub code: Option<String>,
    pub state: Option<String>,
    pub iss: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

pub(crate) struct PendingMcpOAuthSession {
    redirect_uri: String,
    expected_issuer: String,
    require_callback_iss: bool,
    manager: AuthorizationManager,
    callback_rx: oneshot::Receiver<OAuthCallbackParams>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    listener_task: Option<tokio::task::JoinHandle<()>>,
}

impl std::fmt::Debug for PendingMcpOAuthSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingMcpOAuthSession")
            .field("redirect_uri", &self.redirect_uri)
            .field("expected_issuer", &self.expected_issuer)
            .field("require_callback_iss", &self.require_callback_iss)
            .finish_non_exhaustive()
    }
}

impl PendingMcpOAuthSession {
    pub(crate) async fn poll(&mut self) -> Result<PollAuthorizationResult, String> {
        match self.callback_rx.try_recv() {
            Ok(callback) => {
                let (code, state) = validate_callback_payload(
                    &callback,
                    &self.expected_issuer,
                    self.require_callback_iss,
                )?;
                self.manager
                    .exchange_code_for_token(&code, &state)
                    .await
                    .map_err(|err| format!("oauth token exchange failed: {err}"))?;
                Ok(PollAuthorizationResult::Completed {
                    message: "OAuth authorization completed. Restart otto to use the server."
                        .into(),
                })
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                Ok(PollAuthorizationResult::Pending {
                    redirect_uri: self.redirect_uri.clone(),
                })
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                Ok(PollAuthorizationResult::Failed {
                    message: "oauth callback listener closed before a callback arrived".into(),
                })
            }
        }
    }

    pub(crate) async fn shutdown(mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        if let Some(listener_task) = self.listener_task.take() {
            listener_task.abort();
            let _ = listener_task.await;
        }
    }
}

impl Drop for PendingMcpOAuthSession {
    fn drop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        if let Some(listener_task) = self.listener_task.take() {
            listener_task.abort();
        }
    }
}

#[derive(Debug, Serialize)]
struct DynamicClientRegistrationRequest {
    client_name: String,
    redirect_uris: Vec<String>,
    grant_types: Vec<String>,
    token_endpoint_auth_method: String,
    response_types: Vec<String>,
    application_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DynamicClientRegistrationResponse {
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
}

pub(crate) async fn begin_authorization(
    server_name: &str,
    resource_url: &str,
    requested_scopes: &[String],
) -> Result<(PendingMcpOAuthSession, BeginAuthorizationResult), String> {
    validate_protected_resource_url(resource_url)?;
    let mut manager = AuthorizationManager::new(resource_url)
        .await
        .map_err(|err| format!("failed to prepare oauth manager: {err}"))?;
    let metadata = manager
        .discover_metadata()
        .await
        .map_err(|err| format!("oauth metadata discovery failed: {err}"))?;
    let issuer = metadata
        .issuer
        .clone()
        .ok_or_else(|| "authorization server metadata is missing issuer".to_string())?;
    validate_discovered_metadata(&validation_probe_secret(&issuer), &metadata)?;
    let require_callback_iss = authorization_response_iss_required(&metadata);
    manager.set_metadata(metadata.clone());

    let existing = load_stored_secret(server_name)?;
    let discovered_scopes = manager.select_scopes(None, &[]);
    let requested_scopes = merge_requested_scopes(
        existing
            .as_ref()
            .map(|secret| secret.requested_scopes.as_slice())
            .unwrap_or(&[]),
        &discovered_scopes,
        requested_scopes,
    );

    let reuse_candidate = existing
        .as_ref()
        .filter(|secret| secret.issuer == issuer && !secret.client_id.trim().is_empty())
        .cloned();
    let (listener, client_config, persisted_secret, reused_client_registration) =
        if let Some(secret) = reuse_candidate {
            match start_callback_listener(Some(&secret.redirect_uri)).await {
                Ok(listener) => {
                    let mut persisted = secret.clone();
                    persisted.requested_scopes = requested_scopes.clone();
                    let mut client_config =
                        OAuthClientConfig::new(secret.client_id.clone(), secret.redirect_uri);
                    if let Some(client_secret) = secret.client_secret.clone() {
                        client_config = client_config.with_client_secret(client_secret);
                    }
                    client_config = client_config.with_scopes(requested_scopes.clone());
                    (listener, client_config, persisted, true)
                }
                Err(_) => {
                    build_registered_client(
                        server_name,
                        metadata.registration_endpoint.as_deref(),
                        issuer.as_str(),
                        requested_scopes.clone(),
                    )
                    .await?
                }
            }
        } else {
            build_registered_client(
                server_name,
                metadata.registration_endpoint.as_deref(),
                issuer.as_str(),
                requested_scopes.clone(),
            )
            .await?
        };

    creds::mcp_save_json(server_name, &persisted_secret).map_err(|err| {
        format!("failed to persist oauth keyring state for `mcp:{server_name}`: {err}")
    })?;

    manager.set_state_store(InMemoryStateStore::new());
    manager.set_credential_store(KeyringOAuthCredentialStore::new(
        server_name,
        persisted_secret.clone(),
    ));
    manager
        .configure_client(client_config)
        .map_err(|err| format!("oauth client configuration failed: {err}"))?;
    let scope_refs = requested_scopes
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let authorization_url = manager
        .get_authorization_url(&scope_refs)
        .await
        .map_err(|err| format!("failed to build authorization URL: {err}"))?;

    Ok((
        PendingMcpOAuthSession {
            redirect_uri: listener.redirect_uri.clone(),
            expected_issuer: issuer,
            require_callback_iss,
            manager,
            callback_rx: listener.callback_rx,
            shutdown_tx: Some(listener.shutdown_tx),
            listener_task: Some(listener.task),
        },
        BeginAuthorizationResult {
            authorization_url,
            redirect_uri: listener.redirect_uri,
            reused_client_registration,
        },
    ))
}

struct StartedCallbackListener {
    redirect_uri: String,
    callback_rx: oneshot::Receiver<OAuthCallbackParams>,
    shutdown_tx: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl StartedCallbackListener {
    async fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
        self.task.abort();
        let _ = self.task.await;
    }
}

fn merge_requested_scopes(
    stored_scopes: &[String],
    discovered_scopes: &[String],
    explicit_scopes: &[String],
) -> Vec<String> {
    let mut merged = Vec::new();
    for scope in stored_scopes
        .iter()
        .chain(discovered_scopes.iter())
        .chain(explicit_scopes.iter())
    {
        if !scope.trim().is_empty() && !merged.iter().any(|existing| existing == scope) {
            merged.push(scope.clone());
        }
    }
    merged
}

async fn build_registered_client(
    server_name: &str,
    registration_endpoint: Option<&str>,
    issuer: &str,
    requested_scopes: Vec<String>,
) -> Result<
    (
        StartedCallbackListener,
        OAuthClientConfig,
        StoredMcpOAuthSecret,
        bool,
    ),
    String,
> {
    let registration_endpoint = registration_endpoint
        .ok_or_else(|| "dynamic client registration is not supported by this server".to_string())?;
    let listener = start_callback_listener(None).await?;
    let redirect_uri = listener.redirect_uri.clone();
    let client = match perform_dynamic_client_registration(
        registration_endpoint,
        &format!("Otto MCP ({server_name})"),
        &redirect_uri,
        &requested_scopes,
    )
    .await
    {
        Ok(client) => client,
        Err(err) => {
            listener.shutdown().await;
            return Err(err);
        }
    };
    let mut config = OAuthClientConfig::new(client.client_id.clone(), redirect_uri.clone())
        .with_scopes(requested_scopes.clone());
    if let Some(client_secret) = client.client_secret.clone() {
        config = config.with_client_secret(client_secret.clone());
    }
    Ok((
        listener,
        config,
        StoredMcpOAuthSecret {
            issuer: issuer.to_string(),
            client_id: client.client_id,
            client_secret: client.client_secret,
            redirect_uri,
            requested_scopes,
            granted_scopes: vec![],
            token_response: None,
            token_received_at: None,
        },
        false,
    ))
}

pub(crate) fn validate_callback_payload(
    callback: &OAuthCallbackParams,
    expected_issuer: &str,
    require_iss: bool,
) -> Result<(String, String), String> {
    if let Some(error) = callback.error.as_deref() {
        return Err(match callback.error_description.as_deref() {
            Some(description) if !description.is_empty() => {
                format!("oauth authorization failed: {error} ({description})")
            }
            _ => format!("oauth authorization failed: {error}"),
        });
    }
    match callback.iss.as_deref() {
        Some(found) if found != expected_issuer => {
            return Err(format!(
                "oauth callback issuer mismatch: expected `{expected_issuer}`, got `{found}`"
            ));
        }
        None if require_iss => {
            return Err("oauth callback was missing `iss`".into());
        }
        _ => {}
    }
    let code = callback
        .code
        .clone()
        .ok_or_else(|| "oauth callback was missing `code`".to_string())?;
    let state = callback
        .state
        .clone()
        .ok_or_else(|| "oauth callback was missing `state`".to_string())?;
    Ok((code, state))
}

async fn start_callback_listener(
    preferred_redirect_uri: Option<&str>,
) -> Result<StartedCallbackListener, String> {
    let (listener, redirect_uri, route_path) = match preferred_redirect_uri {
        Some(uri) => bind_existing_redirect_uri(uri).await?,
        None => {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|err| format!("failed to bind oauth loopback listener: {err}"))?;
            let addr = listener
                .local_addr()
                .map_err(|err| format!("failed to read oauth loopback listener address: {err}"))?;
            (
                listener,
                format!("http://127.0.0.1:{}/oauth/callback", addr.port()),
                "/oauth/callback".to_string(),
            )
        }
    };

    let (callback_tx, callback_rx) = oneshot::channel();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let state = axum::extract::State(CallbackListenerState {
        callback_tx: std::sync::Arc::new(Mutex::new(Some(callback_tx))),
        shutdown_tx: std::sync::Arc::new(Mutex::new(Some(shutdown_tx))),
    });
    let app = axum::Router::new().route(
        route_path.as_str(),
        axum::routing::get(
            |axum::extract::State(state): axum::extract::State<CallbackListenerState>,
             axum::extract::Query(query): axum::extract::Query<OAuthCallbackParams>| async move {
                if let Some(sender) = state.callback_tx.lock().await.take() {
                    let _ = sender.send(query);
                }
                if let Some(shutdown) = state.shutdown_tx.lock().await.take() {
                    let _ = shutdown.send(());
                }
                (
                    axum::http::StatusCode::OK,
                    [(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                    "Otto received the OAuth callback. You can return to the terminal.",
                )
            },
        ),
    )
    .with_state(state.0);
    let (external_shutdown_tx, external_shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                tokio::select! {
                    _ = shutdown_rx => {},
                    _ = external_shutdown_rx => {},
                }
            })
            .await;
    });
    Ok(StartedCallbackListener {
        redirect_uri,
        callback_rx,
        shutdown_tx: external_shutdown_tx,
        task,
    })
}

#[derive(Clone)]
struct CallbackListenerState {
    callback_tx: std::sync::Arc<Mutex<Option<oneshot::Sender<OAuthCallbackParams>>>>,
    shutdown_tx: std::sync::Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

async fn bind_existing_redirect_uri(
    redirect_uri: &str,
) -> Result<(TcpListener, String, String), String> {
    let url = reqwest::Url::parse(redirect_uri)
        .map_err(|err| format!("stored redirect_uri is invalid: {err}"))?;
    if url.scheme() != "http" {
        return Err("stored redirect_uri must use http loopback".into());
    }
    if url.host_str() != Some("127.0.0.1") {
        return Err("stored redirect_uri must bind to 127.0.0.1".into());
    }
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "stored redirect_uri is missing a port".to_string())?;
    let path = if url.path().is_empty() {
        "/".to_string()
    } else {
        url.path().to_string()
    };
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|err| format!("failed to reuse stored redirect_uri `{redirect_uri}`: {err}"))?;
    Ok((listener, redirect_uri.to_string(), path))
}

async fn perform_dynamic_client_registration(
    registration_endpoint: &str,
    client_name: &str,
    redirect_uri: &str,
    requested_scopes: &[String],
) -> Result<DynamicClientRegistrationResponse, String> {
    let request = DynamicClientRegistrationRequest {
        client_name: client_name.to_string(),
        redirect_uris: vec![redirect_uri.to_string()],
        grant_types: vec!["authorization_code".into(), "refresh_token".into()],
        token_endpoint_auth_method: "none".into(),
        response_types: vec!["code".into()],
        application_type: "native".into(),
        scope: (!requested_scopes.is_empty()).then(|| requested_scopes.join(" ")),
    };
    let response = reqwest::Client::new()
        .post(registration_endpoint)
        .json(&request)
        .send()
        .await
        .map_err(|err| format!("dynamic client registration request failed: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("dynamic client registration response read failed: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "dynamic client registration failed with HTTP {status}: {body}"
        ));
    }
    serde_json::from_str(&body)
        .map_err(|err| format!("dynamic client registration response was not valid JSON: {err}"))
}

fn validation_probe_secret(issuer: &str) -> StoredMcpOAuthSecret {
    StoredMcpOAuthSecret {
        issuer: issuer.to_string(),
        client_id: "placeholder".into(),
        client_secret: None,
        redirect_uri: "http://127.0.0.1:1/oauth/callback".into(),
        requested_scopes: vec![],
        granted_scopes: vec![],
        token_response: Some(
            serde_json::from_value(serde_json::json!({
                "access_token": "placeholder",
                "token_type": "Bearer",
            }))
            .expect("placeholder token"),
        ),
        token_received_at: Some(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Bytes,
        http::{HeaderValue, StatusCode, header::CONTENT_TYPE},
        routing::{get, post},
    };
    use serde_json::json;
    use std::{
        net::SocketAddr,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };
    use tokio::net::TcpListener;
    use tokio::sync::Mutex as AsyncMutex;

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

    async fn spawn_oauth_server(
        captured_registration: Arc<AsyncMutex<Option<serde_json::Value>>>,
    ) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let issuer = format!("http://127.0.0.1:{}/issuer", addr.port());
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
                "/.well-known/oauth-authorization-server/issuer",
                get({
                    let issuer = issuer.clone();
                    move || async move {
                        (
                            StatusCode::OK,
                            [(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
                            serde_json::to_string(&json!({
                                "issuer": issuer,
                                "authorization_endpoint": format!("{}/authorize", issuer),
                                "token_endpoint": format!("{}/token", issuer),
                                "registration_endpoint": format!("{}/register", issuer),
                                "response_types_supported": ["code"],
                                "code_challenge_methods_supported": ["S256"],
                            }))
                            .unwrap(),
                        )
                    }
                }),
            )
            .route(
                "/issuer/register",
                post({
                    let captured_registration = Arc::clone(&captured_registration);
                    move |body: Bytes| {
                        let captured_registration = Arc::clone(&captured_registration);
                        async move {
                            *captured_registration.lock().await =
                                Some(serde_json::from_slice(&body).unwrap());
                            (
                                StatusCode::CREATED,
                                [(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
                                serde_json::to_string(&json!({
                                    "client_id": "registered-client",
                                }))
                                .unwrap(),
                            )
                        }
                    }
                }),
            )
            .route(
                "/issuer/token",
                post(|| async move {
                    (
                        StatusCode::OK,
                        [(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
                        serde_json::to_string(&json!({
                            "access_token": "fresh-access-token",
                            "refresh_token": "fresh-refresh-token",
                            "token_type": "Bearer",
                            "expires_in": 3600,
                            "scope": "mcp.read",
                        }))
                        .unwrap(),
                    )
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

    #[test]
    fn discovered_metadata_rejects_non_https_remote_endpoints() {
        let stored = stored_secret("http://evil.example/issuer");
        let metadata: AuthorizationMetadata = serde_json::from_value(json!({
            "issuer": "http://evil.example/issuer",
            "authorization_endpoint": "http://evil.example/issuer/authorize",
            "token_endpoint": "http://evil.example/issuer/token",
            "registration_endpoint": "http://evil.example/issuer/register",
            "response_types_supported": ["code"],
            "code_challenge_methods_supported": ["S256"],
        }))
        .expect("metadata json");
        let err = validate_discovered_metadata(&stored, &metadata).unwrap_err();
        assert!(err.contains("must use https"), "err: {err}");
    }

    #[test]
    fn callback_validation_rejects_issuer_mismatch() {
        let err = validate_callback_payload(
            &OAuthCallbackParams {
                code: Some("code".into()),
                state: Some("state".into()),
                iss: Some("https://wrong.example".into()),
                error: None,
                error_description: None,
            },
            "https://issuer.example",
            true,
        )
        .unwrap_err();
        assert!(err.contains("issuer mismatch"));
    }

    #[test]
    fn callback_validation_allows_missing_iss_when_not_required() {
        let (code, state) = validate_callback_payload(
            &OAuthCallbackParams {
                code: Some("code".into()),
                state: Some("state".into()),
                iss: None,
                error: None,
                error_description: None,
            },
            "https://issuer.example",
            false,
        )
        .expect("callback should validate");
        assert_eq!(code, "code");
        assert_eq!(state, "state");
    }

    #[test]
    fn callback_validation_rejects_missing_iss_when_required() {
        let err = validate_callback_payload(
            &OAuthCallbackParams {
                code: Some("code".into()),
                state: Some("state".into()),
                iss: None,
                error: None,
                error_description: None,
            },
            "https://issuer.example",
            true,
        )
        .unwrap_err();
        assert!(err.contains("missing `iss`"));
    }

    #[test]
    fn callback_validation_rejects_missing_state() {
        let err = validate_callback_payload(
            &OAuthCallbackParams {
                code: Some("code".into()),
                state: None,
                iss: Some("https://issuer.example".into()),
                error: None,
                error_description: None,
            },
            "https://issuer.example",
            true,
        )
        .unwrap_err();
        assert!(err.contains("missing `state`"));
    }

    #[tokio::test]
    async fn dynamic_registration_sends_native_client_request() {
        let captured = Arc::new(AsyncMutex::new(None));
        let (addr, handle) = spawn_oauth_server(Arc::clone(&captured)).await;
        let response = perform_dynamic_client_registration(
            &format!("http://127.0.0.1:{}/issuer/register", addr.port()),
            "Otto MCP (demo)",
            "http://127.0.0.1:43111/oauth/callback",
            &[String::from("mcp.read")],
        )
        .await
        .expect("registration succeeds");
        handle.abort();

        assert_eq!(response.client_id, "registered-client");
        let payload = captured
            .lock()
            .await
            .clone()
            .expect("captured request body");
        assert_eq!(payload["application_type"], "native");
        assert_eq!(payload["token_endpoint_auth_method"], "none");
        assert_eq!(
            payload["redirect_uris"][0],
            "http://127.0.0.1:43111/oauth/callback"
        );
        assert_eq!(payload["scope"], "mcp.read");
    }

    #[tokio::test]
    async fn begin_and_complete_authorization_persists_tokens_when_keyring_is_available() {
        let server_name = format!(
            "oauth-flow-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        );
        if crate::creds::mcp_save(&server_name, "probe").is_err() {
            return;
        }
        let _ = crate::creds::mcp_delete(&server_name);

        let captured = Arc::new(AsyncMutex::new(None));
        let (addr, handle) = spawn_oauth_server(captured).await;
        let issuer = format!("http://127.0.0.1:{}/issuer", addr.port());
        let resource_url = format!("http://127.0.0.1:{}/mcp", addr.port());

        let (mut pending, begin) =
            begin_authorization(&server_name, &resource_url, &[String::from("mcp.read")])
                .await
                .expect("begin authorization");

        let auth_url = reqwest::Url::parse(&begin.authorization_url).expect("auth url");
        let state = auth_url
            .query_pairs()
            .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
            .expect("authorization url should carry state");

        reqwest::get(format!(
            "{}?code=auth-code&state={state}&iss={issuer}",
            begin.redirect_uri
        ))
        .await
        .expect("callback request");

        let mut result = pending.poll().await.expect("poll");
        for _ in 0..20 {
            if !matches!(result, PollAuthorizationResult::Pending { .. }) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            result = pending.poll().await.expect("poll");
        }

        pending.shutdown().await;
        handle.abort();

        assert!(matches!(result, PollAuthorizationResult::Completed { .. }));
        let stored = load_stored_secret(&server_name)
            .expect("load secret")
            .expect("secret saved");
        let json = serde_json::to_value(&stored).expect("serialize stored secret");
        assert_eq!(json["client_id"], "registered-client");
        assert_eq!(json["token_response"]["access_token"], "fresh-access-token");
        assert_eq!(
            json["token_response"]["refresh_token"],
            "fresh-refresh-token"
        );
        let _ = crate::creds::mcp_delete(&server_name);
    }

    #[tokio::test]
    async fn begin_authorization_defaults_scopes_from_discovery_when_keyring_is_available() {
        let server_name = format!(
            "oauth-scopes-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        );
        if crate::creds::mcp_save(&server_name, "probe").is_err() {
            return;
        }
        let _ = crate::creds::mcp_delete(&server_name);

        let captured = Arc::new(AsyncMutex::new(None));
        let (addr, handle) = spawn_oauth_server(captured).await;
        let resource_url = format!("http://127.0.0.1:{}/mcp", addr.port());

        let (pending, begin) = begin_authorization(&server_name, &resource_url, &[])
            .await
            .expect("begin authorization");
        pending.shutdown().await;
        handle.abort();

        let auth_url = reqwest::Url::parse(&begin.authorization_url).expect("auth url");
        let scope = auth_url
            .query_pairs()
            .find_map(|(key, value)| (key == "scope").then(|| value.into_owned()))
            .expect("authorization url should carry derived scope");
        assert_eq!(scope, "mcp.read");

        let stored = load_stored_secret(&server_name)
            .expect("load secret")
            .expect("secret saved");
        assert_eq!(stored.requested_scopes, vec!["mcp.read".to_string()]);
        let _ = crate::creds::mcp_delete(&server_name);
    }
}
