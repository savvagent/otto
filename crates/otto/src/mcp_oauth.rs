use otto_host::HttpAuth;
use rmcp::transport::auth::{
    AuthClient, AuthError, AuthorizationManager, AuthorizationMetadata, CredentialStore,
    InMemoryStateStore, OAuthClientConfig, OAuthTokenResponse, StoredCredentials,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, oneshot};
use tokio::time::Duration;
use uuid::Uuid;

use crate::creds;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct StoredMcpOAuthSecret {
    pub issuer: String,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    pub redirect_uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registration_endpoint: Option<String>,
    #[serde(default)]
    pub registration_version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registration_instance_id: Option<String>,
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
            .field("authorization_endpoint", &self.authorization_endpoint)
            .field("token_endpoint", &self.token_endpoint)
            .field("registration_endpoint", &self.registration_endpoint)
            .field("registration_version", &self.registration_version)
            .field("registration_instance_id", &self.registration_instance_id)
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
    pub(crate) fn validate_for_startup(&self) -> Result<(), String> {
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

enum StoredStateDisposition {
    AllowWrite,
    IgnoreBecauseNewer,
    RejectBecauseConcurrentFlow,
}

fn stored_state_disposition(
    existing: &StoredMcpOAuthSecret,
    candidate: &StoredMcpOAuthSecret,
) -> StoredStateDisposition {
    if existing.registration_version != candidate.registration_version {
        return if existing.registration_version > candidate.registration_version {
            StoredStateDisposition::IgnoreBecauseNewer
        } else {
            StoredStateDisposition::AllowWrite
        };
    }

    match (
        existing.registration_instance_id.as_deref(),
        candidate.registration_instance_id.as_deref(),
    ) {
        (Some(existing), Some(candidate)) if existing != candidate => {
            StoredStateDisposition::RejectBecauseConcurrentFlow
        }
        (Some(_), None) => StoredStateDisposition::IgnoreBecauseNewer,
        _ => StoredStateDisposition::AllowWrite,
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
        if let Ok(Some(existing)) = creds::mcp_load_json::<StoredMcpOAuthSecret>(&self.server_name)
        {
            match stored_state_disposition(&existing, &self.seed) {
                StoredStateDisposition::AllowWrite => {}
                StoredStateDisposition::IgnoreBecauseNewer => return Ok(()),
                StoredStateDisposition::RejectBecauseConcurrentFlow => {
                    return Err(AuthError::AuthorizationFailed(
                        "another OAuth authorization completed first; retry authorization to apply this session"
                            .to_string(),
                    ));
                }
            }
        }
        let mut secret = self.seed.clone();
        secret.apply_stored_credentials(credentials);
        creds::mcp_save_json(&self.server_name, &secret).map_err(|err| {
            AuthError::InternalError(format!(
                "failed to persist oauth keyring state for `mcp:{}`: {err}",
                self.server_name
            ))
        })
    }

    async fn clear(&self) -> Result<(), AuthError> {
        if let Ok(Some(existing)) = creds::mcp_load_json::<StoredMcpOAuthSecret>(&self.server_name)
        {
            match stored_state_disposition(&existing, &self.seed) {
                StoredStateDisposition::AllowWrite => {}
                StoredStateDisposition::IgnoreBecauseNewer
                | StoredStateDisposition::RejectBecauseConcurrentFlow => return Ok(()),
            }
        }
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
    match creds::mcp_load_json(server_name) {
        Ok(secret) => Ok(secret),
        Err(crate::creds::JsonSecretError::InvalidJson(_)) => Ok(None),
        Err(err) => Err(format!(
            "failed to read oauth keyring state for `mcp:{server_name}`: {err}"
        )),
    }
}

pub(crate) async fn build_startup_http_auth(
    server_name: &str,
    resource_url: &str,
    stored: StoredMcpOAuthSecret,
    timeout: Duration,
) -> Result<HttpAuth, String> {
    let allow_loopback_http = validate_protected_resource_url(resource_url)?;
    let parsed_resource_url = reqwest::Url::parse(resource_url)
        .map_err(|err| format!("oauth-protected MCP resource URL is not a valid URL: {err}"))?;
    stored.validate_for_startup()?;

    let mut manager = AuthorizationManager::new(resource_url)
        .await
        .map_err(|err| format!("failed to prepare oauth manager: {err}"))?;
    let (metadata, _) =
        discover_metadata_with_policy(resource_url, timeout, allow_loopback_http).await?;
    validate_discovered_metadata(
        &stored,
        &metadata,
        &parsed_resource_url,
        allow_loopback_http,
        true,
    )?;
    validate_discovered_metadata_network(
        &stored,
        &metadata,
        &parsed_resource_url,
        allow_loopback_http,
    )
    .await?;

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
    resource_url: &reqwest::Url,
    allow_loopback_http: bool,
    require_endpoint_pins: bool,
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
    validate_authorization_server_url(
        "authorization server issuer",
        &stored.issuer,
        resource_url,
        allow_loopback_http,
    )?;
    if require_endpoint_pins && stored.authorization_endpoint.is_none() {
        return Err("stored oauth authorization endpoint is missing; re-authorize in /mcp".into());
    }
    if let Some(expected) = stored.authorization_endpoint.as_deref()
        && metadata.authorization_endpoint != expected
    {
        return Err(format!(
            "authorization endpoint changed for issuer `{}`; re-authorize in /mcp",
            stored.issuer
        ));
    }
    validate_authorization_server_url(
        "authorization endpoint",
        &metadata.authorization_endpoint,
        resource_url,
        allow_loopback_http,
    )?;
    if require_endpoint_pins && stored.token_endpoint.is_none() {
        return Err("stored oauth token endpoint is missing; re-authorize in /mcp".into());
    }
    if let Some(expected) = stored.token_endpoint.as_deref()
        && metadata.token_endpoint != expected
    {
        return Err(format!(
            "token endpoint changed for issuer `{}`; re-authorize in /mcp",
            stored.issuer
        ));
    }
    validate_authorization_server_url(
        "token endpoint",
        &metadata.token_endpoint,
        resource_url,
        allow_loopback_http,
    )?;
    if let Some(expected) = stored.registration_endpoint.as_deref()
        && let Some(found) = metadata.registration_endpoint.as_deref()
        && found != expected
    {
        return Err(format!(
            "registration endpoint changed for issuer `{}`; re-authorize in /mcp",
            stored.issuer
        ));
    }
    if let Some(registration_endpoint) = metadata.registration_endpoint.as_deref() {
        validate_authorization_server_url(
            "registration endpoint",
            registration_endpoint,
            resource_url,
            allow_loopback_http,
        )?;
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

fn validate_authorization_server_url(
    label: &str,
    url: &str,
    resource_url: &reqwest::Url,
    allow_loopback_http: bool,
) -> Result<(), String> {
    let parsed =
        reqwest::Url::parse(url).map_err(|err| format!("{label} is not a valid URL: {err}"))?;
    if parsed.scheme() == "https" {
        return validate_authorization_server_host(
            label,
            &parsed,
            resource_url,
            allow_loopback_http,
        );
    }
    if allow_loopback_http && parsed.scheme() == "http" && is_loopback_host(parsed.host_str()) {
        return validate_authorization_server_host(
            label,
            &parsed,
            resource_url,
            allow_loopback_http,
        );
    }
    if parsed.scheme() != "https" {
        return Err(format!(
            "{label} must use https (or http loopback for local development): {url}"
        ));
    }
    validate_authorization_server_host(label, &parsed, resource_url, allow_loopback_http)
}

fn validate_protected_resource_url(url: &str) -> Result<bool, String> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|err| format!("oauth-protected MCP resource URL is not a valid URL: {err}"))?;
    if parsed.scheme() == "https" {
        return Ok(false);
    }
    if parsed.scheme() == "http" && is_loopback_host(parsed.host_str()) {
        return Ok(true);
    }
    Err(format!(
        "oauth-protected MCP resource URL must use https (or http loopback for local development): {url}"
    ))
}

fn is_loopback_host(host: Option<&str>) -> bool {
    matches!(host, Some("127.0.0.1" | "localhost" | "::1"))
}

fn validate_authorization_server_host(
    label: &str,
    url: &reqwest::Url,
    resource_url: &reqwest::Url,
    allow_loopback_http: bool,
) -> Result<(), String> {
    if allow_loopback_http || same_origin(resource_url, url) || !is_restricted_host(url.host_str())
    {
        return Ok(());
    }
    Err(format!(
        "{label} must not target localhost or private-network hosts for non-loopback MCP servers: {url}"
    ))
}

async fn validate_authorization_server_dns(
    label: &str,
    url: &reqwest::Url,
    resource_url: &reqwest::Url,
    allow_loopback_http: bool,
) -> Result<(), String> {
    if allow_loopback_http || same_origin(resource_url, url) {
        return Ok(());
    }
    let Some(host) = url.host_str() else {
        return Err(format!("{label} is missing a host: {url}"));
    };
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Ok(());
    }
    let port = url.port_or_known_default().unwrap_or(443);
    let resolved = tokio::net::lookup_host((host, port))
        .await
        .map_err(|err| format!("failed to resolve {label} host `{host}`: {err}"))?;
    for addr in resolved {
        if is_restricted_ip(addr.ip()) {
            return Err(format!(
                "{label} must not resolve to localhost or private-network addresses for non-loopback MCP servers: {url}"
            ));
        }
    }
    Ok(())
}

fn validate_resource_metadata_url(
    resource_url: &reqwest::Url,
    candidate_url: &reqwest::Url,
    allow_loopback_http: bool,
) -> Result<(), String> {
    let same_origin = same_origin(resource_url, candidate_url);
    if same_origin
        && (candidate_url.scheme() == "https"
            || (allow_loopback_http
                && candidate_url.scheme() == "http"
                && is_loopback_host(candidate_url.host_str())))
    {
        return Ok(());
    }
    Err(format!(
        "resource metadata endpoint must stay on the MCP server origin: {candidate_url}"
    ))
}

fn same_origin(left: &reqwest::Url, right: &reqwest::Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

fn is_restricted_host(host: Option<&str>) -> bool {
    let Some(host) = host else {
        return true;
    };
    if matches!(host, "localhost" | "metadata.google.internal") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => is_restricted_ip(ip),
        Err(_) => false,
    }
}

fn is_restricted_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.octets() == [100, 100, 100, 200]
        }
        std::net::IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
        }
    }
}

#[derive(Debug, Default)]
struct DiscoveryHints {
    www_authenticate_scope: Option<String>,
    resource_scopes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ProtectedResourceMetadata {
    #[serde(default)]
    authorization_server: Option<String>,
    #[serde(default)]
    authorization_servers: Option<Vec<String>>,
    #[serde(default)]
    scopes_supported: Option<Vec<String>>,
}

async fn discover_metadata_with_policy(
    resource_url: &str,
    timeout: Duration,
    allow_loopback_http: bool,
) -> Result<(AuthorizationMetadata, DiscoveryHints), String> {
    let base_url = reqwest::Url::parse(resource_url)
        .map_err(|err| format!("oauth-protected MCP resource URL is not a valid URL: {err}"))?;
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|err| format!("failed to build oauth discovery client: {err}"))?;

    if let Some(result) =
        try_discover_via_resource_metadata(&client, &base_url, allow_loopback_http).await?
    {
        return Ok(result);
    }

    try_discover_authorization_server(&client, &base_url, &base_url, allow_loopback_http)
        .await?
        .ok_or_else(|| "oauth metadata discovery failed: authorization not supported".to_string())
}

async fn try_discover_via_resource_metadata(
    client: &reqwest::Client,
    base_url: &reqwest::Url,
    allow_loopback_http: bool,
) -> Result<Option<(AuthorizationMetadata, DiscoveryHints)>, String> {
    let mut hints = DiscoveryHints::default();
    let mut candidate_urls = Vec::new();

    let response = match fetch_discovery_response(client, base_url.clone()).await {
        Ok(Some(response)) => Some(response),
        Ok(None) => None,
        Err(err) => return Err(err),
    };

    if let Some(response) = response {
        match response.status() {
            reqwest::StatusCode::OK => {
                candidate_urls.push(base_url.clone());
            }
            reqwest::StatusCode::UNAUTHORIZED => {
                for value in response
                    .headers()
                    .get_all(reqwest::header::WWW_AUTHENTICATE)
                    .iter()
                {
                    let Ok(value) = value.to_str() else {
                        continue;
                    };
                    if hints.www_authenticate_scope.is_none() {
                        hints.www_authenticate_scope =
                            extract_www_authenticate_param(value, "scope");
                    }
                    if let Some(url) = extract_www_authenticate_resource_metadata(value, base_url)?
                    {
                        validate_resource_metadata_url(base_url, &url, allow_loopback_http)?;
                        candidate_urls.push(url);
                        break;
                    }
                }
            }
            _ => {}
        }
    }

    for path in well_known_paths(base_url.path(), "oauth-protected-resource") {
        let mut url = base_url.clone();
        url.set_query(None);
        url.set_fragment(None);
        url.set_path(&path);
        candidate_urls.push(url);
    }

    for candidate_url in candidate_urls {
        validate_resource_metadata_url(base_url, &candidate_url, allow_loopback_http)?;
        let Some(resource_metadata) =
            fetch_json::<ProtectedResourceMetadata>(client, candidate_url.clone()).await?
        else {
            continue;
        };
        if let Some(scopes) = resource_metadata.scopes_supported.as_ref()
            && hints.resource_scopes.is_empty()
        {
            hints.resource_scopes = scopes.clone();
        }
        for auth_server in authorization_server_candidates(&resource_metadata, &candidate_url) {
            if let Some(metadata) =
                discover_authorization_metadata(client, &auth_server, base_url, allow_loopback_http)
                    .await?
            {
                return Ok(Some((metadata, hints)));
            }
        }
    }

    Ok(None)
}

async fn try_discover_authorization_server(
    client: &reqwest::Client,
    base_url: &reqwest::Url,
    resource_url: &reqwest::Url,
    allow_loopback_http: bool,
) -> Result<Option<(AuthorizationMetadata, DiscoveryHints)>, String> {
    let Some(metadata) =
        discover_authorization_metadata(client, base_url, resource_url, allow_loopback_http)
            .await?
    else {
        return Ok(None);
    };
    Ok(Some((metadata, DiscoveryHints::default())))
}

async fn discover_authorization_metadata(
    client: &reqwest::Client,
    base_url: &reqwest::Url,
    resource_url: &reqwest::Url,
    allow_loopback_http: bool,
) -> Result<Option<AuthorizationMetadata>, String> {
    for discovery_url in generate_authorization_discovery_urls(base_url) {
        validate_authorization_server_url(
            "authorization metadata endpoint",
            discovery_url.as_str(),
            resource_url,
            allow_loopback_http,
        )?;
        validate_authorization_server_dns(
            "authorization metadata endpoint",
            &discovery_url,
            resource_url,
            allow_loopback_http,
        )
        .await?;
        if let Some(metadata) = fetch_json(client, discovery_url).await? {
            return Ok(Some(metadata));
        }
    }
    Ok(None)
}

async fn validate_discovered_metadata_network(
    stored: &StoredMcpOAuthSecret,
    metadata: &AuthorizationMetadata,
    resource_url: &reqwest::Url,
    allow_loopback_http: bool,
) -> Result<(), String> {
    let issuer = reqwest::Url::parse(&stored.issuer)
        .map_err(|err| format!("authorization server issuer is not a valid URL: {err}"))?;
    validate_authorization_server_dns(
        "authorization server issuer",
        &issuer,
        resource_url,
        allow_loopback_http,
    )
    .await?;

    let authorization_endpoint = reqwest::Url::parse(&metadata.authorization_endpoint)
        .map_err(|err| format!("authorization endpoint is not a valid URL: {err}"))?;
    validate_authorization_server_dns(
        "authorization endpoint",
        &authorization_endpoint,
        resource_url,
        allow_loopback_http,
    )
    .await?;

    let token_endpoint = reqwest::Url::parse(&metadata.token_endpoint)
        .map_err(|err| format!("token endpoint is not a valid URL: {err}"))?;
    validate_authorization_server_dns(
        "token endpoint",
        &token_endpoint,
        resource_url,
        allow_loopback_http,
    )
    .await?;

    if let Some(registration_endpoint) = metadata.registration_endpoint.as_deref() {
        let registration_endpoint = reqwest::Url::parse(registration_endpoint)
            .map_err(|err| format!("registration endpoint is not a valid URL: {err}"))?;
        validate_authorization_server_dns(
            "registration endpoint",
            &registration_endpoint,
            resource_url,
            allow_loopback_http,
        )
        .await?;
    }

    Ok(())
}

async fn fetch_discovery_response(
    client: &reqwest::Client,
    url: reqwest::Url,
) -> Result<Option<reqwest::Response>, String> {
    client
        .get(url)
        .header("MCP-Protocol-Version", "2024-11-05")
        .send()
        .await
        .map(Some)
        .or_else(|err| {
            if err.is_connect() || err.is_timeout() || err.is_request() {
                Ok(None)
            } else {
                Err(format!("oauth metadata discovery request failed: {err}"))
            }
        })
}

async fn fetch_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: reqwest::Url,
) -> Result<Option<T>, String> {
    let Some(response) = fetch_discovery_response(client, url).await? else {
        return Ok(None);
    };
    if response.status() != reqwest::StatusCode::OK {
        return Ok(None);
    }
    let body = response
        .text()
        .await
        .map_err(|err| format!("oauth metadata discovery response read failed: {err}"))?;
    serde_json::from_str(&body).map(Some).or_else(|err| {
        if err.is_syntax() || err.is_data() {
            Ok(None)
        } else {
            Err(format!(
                "oauth metadata discovery response parse failed: {err}"
            ))
        }
    })
}

fn generate_authorization_discovery_urls(base_url: &reqwest::Url) -> Vec<reqwest::Url> {
    let mut candidates = Vec::new();
    let path = base_url.path();
    let trimmed = path.trim_start_matches('/').trim_end_matches('/');
    let mut push_candidate = |discovery_path: String| {
        let mut discovery_url = base_url.clone();
        discovery_url.set_query(None);
        discovery_url.set_fragment(None);
        discovery_url.set_path(&discovery_path);
        candidates.push(discovery_url);
    };
    if trimmed.is_empty() {
        push_candidate("/.well-known/oauth-authorization-server".to_string());
        push_candidate("/.well-known/openid-configuration".to_string());
    } else {
        push_candidate(format!("/.well-known/oauth-authorization-server/{trimmed}"));
        push_candidate(format!("/.well-known/openid-configuration/{trimmed}"));
        push_candidate(format!("/{trimmed}/.well-known/openid-configuration"));
        push_candidate("/.well-known/oauth-authorization-server".to_string());
    }
    candidates
}

fn well_known_paths(path: &str, suffix: &str) -> Vec<String> {
    let mut candidates = vec![format!("/.well-known/{suffix}")];
    let trimmed = path.trim_start_matches('/').trim_end_matches('/');
    if !trimmed.is_empty() {
        candidates.insert(0, format!("/.well-known/{suffix}/{trimmed}"));
        candidates.push(format!("/{trimmed}/.well-known/{suffix}"));
    }
    candidates
}

fn authorization_server_candidates(
    metadata: &ProtectedResourceMetadata,
    resource_metadata_url: &reqwest::Url,
) -> Vec<reqwest::Url> {
    let mut candidates = Vec::new();
    let mut push_candidate = |candidate: &str| {
        let candidate = candidate.trim();
        if candidate.is_empty() {
            return;
        }
        if let Ok(url) = reqwest::Url::parse(candidate) {
            candidates.push(url);
        } else if let Ok(url) = resource_metadata_url.join(candidate) {
            candidates.push(url);
        }
    };
    if let Some(single) = metadata.authorization_server.as_deref() {
        push_candidate(single);
    }
    if let Some(list) = metadata.authorization_servers.as_ref() {
        for item in list {
            push_candidate(item);
        }
    }
    candidates
}

fn extract_www_authenticate_resource_metadata(
    header: &str,
    base_url: &reqwest::Url,
) -> Result<Option<reqwest::Url>, String> {
    let Some(value) = extract_www_authenticate_param(header, "resource_metadata") else {
        return Ok(None);
    };
    if let Ok(url) = reqwest::Url::parse(&value) {
        return Ok(Some(url));
    }
    base_url
        .join(&value)
        .map(Some)
        .map_err(|err| format!("failed to resolve resource metadata URL `{value}`: {err}"))
}

fn extract_www_authenticate_param(header: &str, key: &str) -> Option<String> {
    let header_lowercase = header.to_ascii_lowercase();
    let search = format!("{key}=");
    let pos = header_lowercase.find(&search)?;
    let value_slice = &header[pos + search.len()..];
    parse_next_header_value(value_slice).map(|(value, _)| value)
}

fn parse_next_header_value(header_fragment: &str) -> Option<(String, usize)> {
    let trimmed = header_fragment.trim_start();
    let leading_ws = header_fragment.len() - trimmed.len();

    if let Some(stripped) = trimmed.strip_prefix('"') {
        let mut escaped = false;
        let mut result = String::new();
        for (idx, ch) in stripped.char_indices() {
            if escaped {
                result.push(ch);
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => return Some((result, leading_ws + idx + 2)),
                _ => result.push(ch),
            }
        }
        None
    } else {
        let end = trimmed
            .find(|c: char| c == ',' || c == ';' || c.is_whitespace())
            .unwrap_or(trimmed.len());
        if end == 0 {
            None
        } else {
            Some((trimmed[..end].to_string(), leading_ws + end))
        }
    }
}

fn select_discovered_scopes(
    hints: &DiscoveryHints,
    metadata: &AuthorizationMetadata,
) -> Vec<String> {
    let mut scopes = if let Some(scope) = hints.www_authenticate_scope.as_deref() {
        scope
            .split_whitespace()
            .map(|scope| scope.to_string())
            .collect()
    } else if !hints.resource_scopes.is_empty() {
        hints.resource_scopes.clone()
    } else {
        metadata.scopes_supported.clone().unwrap_or_default()
    };

    if !scopes.is_empty()
        && !scopes.iter().any(|scope| scope == "offline_access")
        && metadata
            .scopes_supported
            .as_ref()
            .is_some_and(|supported| supported.iter().any(|scope| scope == "offline_access"))
    {
        scopes.push("offline_access".to_string());
    }

    scopes
}

fn authorization_response_iss_required(metadata: &AuthorizationMetadata) -> bool {
    metadata
        .additional_fields
        .get("authorization_response_iss_parameter_supported")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn next_registration_version(existing: Option<&StoredMcpOAuthSecret>) -> u64 {
    existing
        .map(|secret| secret.registration_version.saturating_add(1))
        .unwrap_or(1)
}

fn next_registration_instance_id() -> String {
    Uuid::new_v4().to_string()
}

fn sanitize_remote_oauth_text(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_control()).collect()
}

fn summarize_remote_response_body(body: &str) -> String {
    let sanitized = sanitize_remote_oauth_text(body);
    let preview = sanitized.chars().take(200).collect::<String>();
    if sanitized.chars().count() > 200 {
        format!("{preview}…")
    } else {
        preview
    }
}

pub(crate) fn default_network_timeout() -> Duration {
    Duration::from_secs(30)
}

pub(crate) fn startup_network_timeout(connect_timeout: Duration) -> Duration {
    connect_timeout.min(default_network_timeout())
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
    oauth_timeout: Duration,
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
            .field("oauth_timeout", &self.oauth_timeout)
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
                tokio::time::timeout(
                    self.oauth_timeout,
                    self.manager.exchange_code_for_token(&code, &state),
                )
                .await
                .map_err(|_| {
                    format!(
                        "oauth token exchange timed out after {}s",
                        self.oauth_timeout.as_secs()
                    )
                })?
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
    timeout: Duration,
) -> Result<(PendingMcpOAuthSession, BeginAuthorizationResult), String> {
    let allow_loopback_http = validate_protected_resource_url(resource_url)?;
    let parsed_resource_url = reqwest::Url::parse(resource_url)
        .map_err(|err| format!("oauth-protected MCP resource URL is not a valid URL: {err}"))?;
    let mut manager = AuthorizationManager::new(resource_url)
        .await
        .map_err(|err| format!("failed to prepare oauth manager: {err}"))?;
    let (metadata, discovery_hints) =
        discover_metadata_with_policy(resource_url, timeout, allow_loopback_http).await?;
    let existing = load_stored_secret(server_name)?;
    let issuer = metadata
        .issuer
        .clone()
        .ok_or_else(|| "authorization server metadata is missing issuer".to_string())?;
    let validation_secret = existing
        .as_ref()
        .filter(|secret| secret.issuer == issuer)
        .cloned()
        .unwrap_or_else(|| validation_probe_secret(&issuer));
    validate_discovered_metadata(
        &validation_secret,
        &metadata,
        &parsed_resource_url,
        allow_loopback_http,
        false,
    )?;
    validate_discovered_metadata_network(
        &validation_secret,
        &metadata,
        &parsed_resource_url,
        allow_loopback_http,
    )
    .await?;
    let require_callback_iss = authorization_response_iss_required(&metadata);
    manager.set_metadata(metadata.clone());
    let discovered_scopes = select_discovered_scopes(&discovery_hints, &metadata);
    let requested_scopes = merge_requested_scopes(
        existing
            .as_ref()
            .map(|secret| secret.requested_scopes.as_slice())
            .unwrap_or(&[]),
        &discovered_scopes,
        requested_scopes,
    );
    let registration_version = next_registration_version(existing.as_ref());
    let registration_instance_id = next_registration_instance_id();

    let reuse_candidate = existing
        .as_ref()
        .filter(|secret| secret.issuer == issuer && !secret.client_id.trim().is_empty())
        .cloned();
    let (listener, client_config, persisted_secret, reused_client_registration) =
        if let Some(secret) = reuse_candidate {
            match start_callback_listener(Some(&secret.redirect_uri)).await {
                Ok(listener) => {
                    let mut persisted = secret.clone();
                    persisted.authorization_endpoint =
                        Some(metadata.authorization_endpoint.clone());
                    persisted.token_endpoint = Some(metadata.token_endpoint.clone());
                    persisted.registration_endpoint = metadata.registration_endpoint.clone();
                    persisted.registration_version = registration_version;
                    persisted.registration_instance_id = Some(registration_instance_id.clone());
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
                    build_registered_client(RegisteredClientParams {
                        server_name,
                        registration_endpoint: metadata.registration_endpoint.as_deref(),
                        issuer: issuer.as_str(),
                        authorization_endpoint: metadata.authorization_endpoint.clone(),
                        token_endpoint: metadata.token_endpoint.clone(),
                        discovered_registration_endpoint: metadata.registration_endpoint.clone(),
                        requested_scopes: requested_scopes.clone(),
                        registration_version,
                        registration_instance_id: registration_instance_id.clone(),
                        timeout,
                    })
                    .await?
                }
            }
        } else {
            build_registered_client(RegisteredClientParams {
                server_name,
                registration_endpoint: metadata.registration_endpoint.as_deref(),
                issuer: issuer.as_str(),
                authorization_endpoint: metadata.authorization_endpoint.clone(),
                token_endpoint: metadata.token_endpoint.clone(),
                discovered_registration_endpoint: metadata.registration_endpoint.clone(),
                requested_scopes: requested_scopes.clone(),
                registration_version,
                registration_instance_id,
                timeout,
            })
            .await?
        };

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
            oauth_timeout: timeout,
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

struct RegisteredClientParams<'a> {
    server_name: &'a str,
    registration_endpoint: Option<&'a str>,
    issuer: &'a str,
    authorization_endpoint: String,
    token_endpoint: String,
    discovered_registration_endpoint: Option<String>,
    requested_scopes: Vec<String>,
    registration_version: u64,
    registration_instance_id: String,
    timeout: Duration,
}

async fn build_registered_client(
    params: RegisteredClientParams<'_>,
) -> Result<
    (
        StartedCallbackListener,
        OAuthClientConfig,
        StoredMcpOAuthSecret,
        bool,
    ),
    String,
> {
    let RegisteredClientParams {
        server_name,
        registration_endpoint,
        issuer,
        authorization_endpoint,
        token_endpoint,
        discovered_registration_endpoint,
        requested_scopes,
        registration_version,
        registration_instance_id,
        timeout,
    } = params;
    let registration_endpoint = registration_endpoint
        .ok_or_else(|| "dynamic client registration is not supported by this server".to_string())?;
    let listener = start_callback_listener(None).await?;
    let redirect_uri = listener.redirect_uri.clone();
    let client = match perform_dynamic_client_registration(
        registration_endpoint,
        &format!("Otto MCP ({server_name})"),
        &redirect_uri,
        &requested_scopes,
        timeout,
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
            authorization_endpoint: Some(authorization_endpoint),
            token_endpoint: Some(token_endpoint),
            registration_endpoint: discovered_registration_endpoint,
            registration_version,
            registration_instance_id: Some(registration_instance_id),
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
                format!(
                    "oauth authorization failed: {} ({})",
                    sanitize_remote_oauth_text(error),
                    sanitize_remote_oauth_text(description)
                )
            }
            _ => format!(
                "oauth authorization failed: {}",
                sanitize_remote_oauth_text(error)
            ),
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
    timeout: Duration,
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
    let response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .map_err(|err| format!("failed to build oauth registration client: {err}"))?
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
            "dynamic client registration failed with HTTP {status}: {}",
            summarize_remote_response_body(&body)
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
        authorization_endpoint: None,
        token_endpoint: None,
        registration_endpoint: None,
        registration_version: 0,
        registration_instance_id: None,
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
        time::{Duration as StdDuration, SystemTime, UNIX_EPOCH},
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
            authorization_endpoint: Some(format!("{issuer}/authorize")),
            token_endpoint: Some(format!("{issuer}/token")),
            registration_endpoint: Some(format!("{issuer}/register")),
            registration_version: 1,
            registration_instance_id: Some("instance-1".into()),
            requested_scopes: vec!["mcp.read".into()],
            granted_scopes: vec!["mcp.read".into()],
            token_response: Some(make_token_response("access-token", Some(3600))),
            token_received_at: Some(1),
        }
    }

    #[test]
    fn next_registration_version_starts_at_one_and_increments_existing_state() {
        assert_eq!(next_registration_version(None), 1);

        let mut secret = stored_secret("https://issuer.example");
        secret.registration_version = 41;
        assert_eq!(next_registration_version(Some(&secret)), 42);
    }

    #[test]
    fn stored_state_disposition_rejects_same_generation_from_other_flow() {
        let mut existing = stored_secret("https://issuer.example");
        existing.registration_version = 7;
        existing.registration_instance_id = Some("flow-a".into());

        let mut candidate = existing.clone();
        candidate.registration_instance_id = Some("flow-b".into());
        assert!(matches!(
            stored_state_disposition(&existing, &candidate),
            StoredStateDisposition::RejectBecauseConcurrentFlow
        ));

        candidate.registration_instance_id = Some("flow-a".into());
        assert!(matches!(
            stored_state_disposition(&existing, &candidate),
            StoredStateDisposition::AllowWrite
        ));
    }

    #[test]
    fn resource_metadata_url_must_stay_same_origin() {
        let resource = reqwest::Url::parse("https://mcp.example.test/mcp").unwrap();
        let same_origin = reqwest::Url::parse(
            "https://mcp.example.test/.well-known/oauth-protected-resource/mcp",
        )
        .unwrap();
        let other_origin = reqwest::Url::parse(
            "https://auth.example.test/.well-known/oauth-protected-resource/mcp",
        )
        .unwrap();

        assert!(validate_resource_metadata_url(&resource, &same_origin, false).is_ok());
        assert!(
            validate_resource_metadata_url(&resource, &other_origin, false)
                .unwrap_err()
                .contains("must stay on the MCP server origin")
        );
    }

    #[test]
    fn remote_authorization_endpoints_reject_private_https_hosts() {
        let resource_url = reqwest::Url::parse("https://mcp.example.test/mcp").unwrap();
        let err = validate_authorization_server_url(
            "authorization endpoint",
            "https://127.0.0.1/oauth/authorize",
            &resource_url,
            false,
        )
        .unwrap_err();
        assert!(err.contains("must not target localhost or private-network hosts"));
    }

    #[test]
    fn configured_private_ip_origin_is_still_allowed() {
        let resource_url = reqwest::Url::parse("https://10.0.0.5/mcp").unwrap();
        assert!(
            validate_resource_metadata_url(
                &resource_url,
                &reqwest::Url::parse("https://10.0.0.5/.well-known/oauth-protected-resource/mcp")
                    .unwrap(),
                false,
            )
            .is_ok()
        );
        assert!(
            validate_authorization_server_url(
                "authorization endpoint",
                "https://10.0.0.5/oauth/authorize",
                &resource_url,
                false,
            )
            .is_ok()
        );
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
            StdDuration::from_secs(5),
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
            StdDuration::from_secs(5),
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
            StdDuration::from_secs(5),
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
        let resource_url = reqwest::Url::parse("https://mcp.example.test/mcp").unwrap();
        let err = validate_discovered_metadata(&stored, &metadata, &resource_url, false, false)
            .unwrap_err();
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
            StdDuration::from_secs(5),
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

        let (mut pending, begin) = begin_authorization(
            &server_name,
            &resource_url,
            &[String::from("mcp.read")],
            StdDuration::from_secs(5),
        )
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

        let (pending, begin) =
            begin_authorization(&server_name, &resource_url, &[], StdDuration::from_secs(5))
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

        let stored = load_stored_secret(&server_name).expect("load secret");
        assert!(
            stored.is_none(),
            "keyring state should not be rewritten before authorization completes"
        );
        let _ = crate::creds::mcp_delete(&server_name);
    }
}
