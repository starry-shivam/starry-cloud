use std::time::Duration;

use openidconnect::core::{
    CoreAuthenticationFlow, CoreClient, CoreClientAuthMethod, CoreJsonWebKeySet,
    CoreProviderMetadata,
};
use openidconnect::{
    AuthType, AuthorizationCode, ClientId, ClientSecret, CsrfToken, Nonce, OAuth2TokenResponse,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use tokio::sync::OnceCell;

use crate::config::AuthConfig;

pub struct OidcProvider {
    metadata: OnceCell<CoreProviderMetadata>,
    http: reqwest::Client,
}

pub struct OidcAuthorization {
    pub url: String,
    pub state: String,
    pub nonce: String,
    pub verifier: String,
}

pub struct OidcIdentity {
    pub subject: String,
    pub email: Option<String>,
    pub email_verified: bool,
}

impl OidcProvider {
    pub fn new() -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| format!("could not configure OIDC HTTP client: {error}"))?;
        Ok(Self {
            metadata: OnceCell::new(),
            http,
        })
    }

    async fn metadata(&self, auth: &AuthConfig) -> Result<&CoreProviderMetadata, String> {
        self.metadata
            .get_or_try_init(|| async {
                let discovery_url = auth
                    .oidc_discovery_url
                    .as_deref()
                    .ok_or_else(|| "OIDC discovery URL is missing".to_owned())?;
                let response = self
                    .http
                    .get(discovery_url)
                    .send()
                    .await
                    .map_err(|error| format!("OIDC discovery request failed: {error}"))?
                    .error_for_status()
                    .map_err(|error| format!("OIDC discovery returned an error: {error}"))?;
                let metadata = serde_json::from_slice::<CoreProviderMetadata>(
                    &response.bytes().await.map_err(|error| {
                        format!("could not read OIDC discovery response: {error}")
                    })?,
                )
                .map_err(|error| format!("invalid OIDC discovery response: {error}"))?;

                let jwks_response = self
                    .http
                    .get(metadata.jwks_uri().to_string())
                    .send()
                    .await
                    .map_err(|error| format!("OIDC signing-key request failed: {error}"))?
                    .error_for_status()
                    .map_err(|error| {
                        format!("OIDC signing-key endpoint returned an error: {error}")
                    })?;
                let jwks = serde_json::from_slice::<CoreJsonWebKeySet>(
                    &jwks_response
                        .bytes()
                        .await
                        .map_err(|error| format!("could not read OIDC signing keys: {error}"))?,
                )
                .map_err(|error| format!("invalid OIDC signing-key response: {error}"))?;

                Ok::<_, String>(metadata.set_jwks(jwks))
            })
            .await
    }

    fn redirect_uri(callback_url: &str) -> Result<RedirectUrl, String> {
        RedirectUrl::new(callback_url.to_owned())
            .map_err(|error| format!("invalid OIDC callback URL: {error}"))
    }

    fn client_id(auth: &AuthConfig) -> Result<ClientId, String> {
        auth.oidc_client_id
            .as_deref()
            .map(|value| ClientId::new(value.to_owned()))
            .ok_or_else(|| "OIDC client ID is missing".to_owned())
    }

    fn client_secret(auth: &AuthConfig) -> Result<ClientSecret, String> {
        auth.oidc_client_secret
            .as_deref()
            .map(|value| ClientSecret::new(value.to_owned()))
            .ok_or_else(|| "OIDC client secret is missing".to_owned())
    }

    async fn metadata_copy(&self, auth: &AuthConfig) -> Result<CoreProviderMetadata, String> {
        Ok(self.metadata(auth).await?.clone())
    }

    fn token_auth_type(metadata: &CoreProviderMetadata) -> AuthType {
        let Some(methods) = metadata.token_endpoint_auth_methods_supported() else {
            return AuthType::BasicAuth;
        };
        if methods.contains(&CoreClientAuthMethod::ClientSecretBasic) {
            AuthType::BasicAuth
        } else if methods.contains(&CoreClientAuthMethod::ClientSecretPost) {
            AuthType::RequestBody
        } else {
            AuthType::BasicAuth
        }
    }

    pub async fn authorization(
        &self,
        auth: &AuthConfig,
        callback_url: &str,
    ) -> Result<OidcAuthorization, String> {
        let metadata = self.metadata_copy(auth).await?;
        let auth_type = Self::token_auth_type(&metadata);
        let client = CoreClient::from_provider_metadata(
            metadata,
            Self::client_id(auth)?,
            Some(Self::client_secret(auth)?),
        )
        .set_redirect_uri(Self::redirect_uri(callback_url)?)
        .set_auth_type(auth_type);
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let mut request = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .set_pkce_challenge(challenge);
        for scope in &auth.oidc_scope {
            if scope != "openid" {
                request = request.add_scope(Scope::new(scope.clone()));
            }
        }
        let (url, state, nonce) = request.url();

        Ok(OidcAuthorization {
            url: url.to_string(),
            state: state.secret().to_owned(),
            nonce: nonce.secret().to_owned(),
            verifier: verifier.secret().to_owned(),
        })
    }

    pub async fn identity(
        &self,
        auth: &AuthConfig,
        callback_url: &str,
        code: &str,
        nonce: &str,
        verifier: &str,
    ) -> Result<OidcIdentity, String> {
        let metadata = self.metadata_copy(auth).await?;
        let userinfo_endpoint = metadata.userinfo_endpoint().map(ToString::to_string);
        let auth_type = Self::token_auth_type(&metadata);
        let client = CoreClient::from_provider_metadata(
            metadata,
            Self::client_id(auth)?,
            Some(Self::client_secret(auth)?),
        )
        .set_redirect_uri(Self::redirect_uri(callback_url)?)
        .set_auth_type(auth_type);
        let token = client
            .exchange_code(AuthorizationCode::new(code.to_owned()))
            .map_err(|error| format!("OIDC token exchange setup failed: {error}"))?
            .set_pkce_verifier(PkceCodeVerifier::new(verifier.to_owned()))
            .request_async(&self.http)
            .await
            .map_err(|error| format!("OIDC token exchange failed: {error}"))?;

        let id_token = token
            .id_token()
            .ok_or_else(|| "OIDC response did not include an ID token".to_owned())?;
        let nonce = Nonce::new(nonce.to_owned());
        let claims = id_token
            .claims(&client.id_token_verifier(), &nonce)
            .map_err(|error| format!("OIDC ID token validation failed: {error}"))?;
        let subject = claims.subject().as_str().to_owned();

        let (mut email, mut email_verified) = (
            claims.email().map(|email| email.as_str().to_owned()),
            claims.email_verified().unwrap_or(false),
        );
        if let Some(endpoint) = userinfo_endpoint {
            let response = self
                .http
                .get(endpoint)
                .bearer_auth(token.access_token().secret())
                .send()
                .await
                .map_err(|error| format!("OIDC userinfo request failed: {error}"))?
                .error_for_status()
                .map_err(|error| format!("OIDC userinfo returned an error: {error}"))?;
            let userinfo_bytes = response
                .bytes()
                .await
                .map_err(|error| format!("could not read OIDC userinfo response: {error}"))?;
            let userinfo: serde_json::Value = serde_json::from_slice(&userinfo_bytes)
                .map_err(|error| format!("invalid OIDC userinfo response: {error}"))?;
            if userinfo.get("sub").and_then(serde_json::Value::as_str) != Some(&subject) {
                return Err("OIDC userinfo subject did not match the ID token".to_owned());
            }
            email = userinfo
                .get("email")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .or(email);
            email_verified = userinfo
                .get("email_verified")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(email_verified);
        }

        Ok(OidcIdentity {
            subject,
            email,
            email_verified,
        })
    }
}
