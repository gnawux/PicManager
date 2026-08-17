use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use reqwest::header::{ACCEPT, AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, ORIGIN, USER_AGENT};
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::token_store::GarminTokens;

const IOS_CLIENT_ID: &str = "GCM_IOS_DARK";
const IOS_SERVICE_URL: &str = "https://mobile.integration.garmin.cn/gcm/ios";
const IOS_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148";
const NATIVE_USER_AGENT: &str = "GCM-Android-5.23";
const NATIVE_X_GARMIN_USER_AGENT: &str = "com.garmin.android.apps.connectmobile/5.23; ; Google/sdk_gphone64_arm64/google; Android/33; Dalvik/2.1.0";
const DI_GRANT_TYPE: &str =
    "https://connectapi.garmin.com/di-oauth2-service/oauth/grant/service_ticket";
const DI_CLIENT_IDS: &[&str] = &[
    "GARMIN_CONNECT_MOBILE_ANDROID_DI_2025Q2",
    "GARMIN_CONNECT_MOBILE_ANDROID_DI_2024Q4",
    "GARMIN_CONNECT_MOBILE_ANDROID_DI",
    "GARMIN_CONNECT_MOBILE_IOS_DI",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GarminErrorCode {
    InvalidCredentials,
    InvalidMfa,
    NetworkError,
    RateLimited,
    SsoContractError,
    TokenStoreError,
}

impl GarminErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidCredentials => "invalid_credentials",
            Self::InvalidMfa => "invalid_mfa",
            Self::NetworkError => "network_error",
            Self::RateLimited => "rate_limited",
            Self::SsoContractError => "sso_contract_error",
            Self::TokenStoreError => "token_store_error",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("Garmin request failed during {phase}")]
pub struct GarminError {
    pub code: GarminErrorCode,
    pub phase: &'static str,
    pub http_status: Option<u16>,
    pub retryable: bool,
}

impl GarminError {
    fn provider(code: GarminErrorCode, phase: &'static str, status: Option<StatusCode>) -> Self {
        Self {
            code,
            phase,
            http_status: status.map(|value| value.as_u16()),
            retryable: matches!(
                code,
                GarminErrorCode::NetworkError | GarminErrorCode::RateLimited
            ),
        }
    }

    fn transport(phase: &'static str, error: &reqwest::Error) -> Self {
        let status = error.status();
        let code = if status == Some(StatusCode::TOO_MANY_REQUESTS) {
            GarminErrorCode::RateLimited
        } else {
            GarminErrorCode::NetworkError
        };
        Self::provider(code, phase, status)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthenticationOutcome {
    Authenticated(GarminTokens),
    MfaRequired,
}

#[derive(Clone, Debug)]
struct Endpoints {
    sso: Url,
    di_token: Url,
    connect_api: Url,
    service_url: String,
}

impl Endpoints {
    fn production() -> Self {
        Self {
            sso: Url::parse("https://sso.garmin.cn/").expect("static Garmin SSO URL"),
            di_token: Url::parse("https://diauth.garmin.cn/di-oauth2-service/oauth/token")
                .expect("static Garmin DI URL"),
            connect_api: Url::parse("https://connectapi.garmin.cn/")
                .expect("static Garmin API URL"),
            service_url: IOS_SERVICE_URL.to_owned(),
        }
    }

    fn validate_production(&self) -> Result<(), GarminError> {
        for (url, host) in [
            (&self.sso, "sso.garmin.cn"),
            (&self.di_token, "diauth.garmin.cn"),
            (&self.connect_api, "connectapi.garmin.cn"),
        ] {
            if url.scheme() != "https" || url.host_str() != Some(host) {
                return Err(GarminError::provider(
                    GarminErrorCode::SsoContractError,
                    "endpoint_validation",
                    None,
                ));
            }
        }
        let service = Url::parse(&self.service_url).map_err(|_| {
            GarminError::provider(
                GarminErrorCode::SsoContractError,
                "endpoint_validation",
                None,
            )
        })?;
        if service.scheme() != "https" || service.host_str() != Some("mobile.integration.garmin.cn")
        {
            return Err(GarminError::provider(
                GarminErrorCode::SsoContractError,
                "endpoint_validation",
                None,
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct GarminClient {
    http: Client,
    endpoints: Endpoints,
}

impl GarminClient {
    pub fn production() -> Result<Self, GarminError> {
        let endpoints = Endpoints::production();
        endpoints.validate_production()?;
        Self::new(endpoints)
    }

    fn new(endpoints: Endpoints) -> Result<Self, GarminError> {
        let http = Client::builder()
            .cookie_store(true)
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| GarminError::transport("client_initialization", &error))?;
        Ok(Self { http, endpoints })
    }

    #[cfg(test)]
    pub(crate) fn for_test(root: Url) -> Result<Self, GarminError> {
        Self::new(Endpoints {
            sso: root.clone(),
            di_token: root.join("di-oauth2-service/oauth/token").map_err(|_| {
                GarminError::provider(
                    GarminErrorCode::SsoContractError,
                    "endpoint_validation",
                    None,
                )
            })?,
            connect_api: root.clone(),
            service_url: root
                .join("gcm/ios")
                .map_err(|_| {
                    GarminError::provider(
                        GarminErrorCode::SsoContractError,
                        "endpoint_validation",
                        None,
                    )
                })?
                .to_string(),
        })
    }

    pub async fn authenticate(
        &self,
        username: &str,
        password: &str,
        mfa_code: Option<&str>,
    ) -> Result<AuthenticationOutcome, GarminError> {
        if username.trim().is_empty() || password.is_empty() {
            return Err(GarminError::provider(
                GarminErrorCode::InvalidCredentials,
                "credential_login",
                None,
            ));
        }

        let login_url = self.endpoints.sso.join("mobile/api/login").map_err(|_| {
            GarminError::provider(GarminErrorCode::SsoContractError, "credential_login", None)
        })?;
        let response = self
            .http
            .post(login_url)
            .query(&[
                ("clientId", IOS_CLIENT_ID),
                ("locale", "en-US"),
                ("service", self.endpoints.service_url.as_str()),
            ])
            .headers(self.mobile_headers())
            .json(&LoginRequest {
                username,
                password,
                remember_me: true,
                captcha_token: "",
            })
            .send()
            .await
            .map_err(|error| GarminError::transport("credential_login", &error))?;
        let status = response.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(GarminError::provider(
                GarminErrorCode::RateLimited,
                "credential_login",
                Some(status),
            ));
        }
        let login = response.json::<SsoResponse>().await.map_err(|_| {
            GarminError::provider(
                GarminErrorCode::SsoContractError,
                "credential_login",
                Some(status),
            )
        })?;

        let ticket = match login.response_type() {
            Some("SUCCESSFUL") => login.service_ticket_id.ok_or_else(|| {
                GarminError::provider(
                    GarminErrorCode::SsoContractError,
                    "credential_login",
                    Some(status),
                )
            })?,
            Some("MFA_REQUIRED") => {
                let Some(code) = mfa_code.filter(|value| !value.trim().is_empty()) else {
                    return Ok(AuthenticationOutcome::MfaRequired);
                };
                self.verify_mfa(code, login.mfa_method()).await?
            }
            Some("INVALID_USERNAME_PASSWORD") => {
                return Err(GarminError::provider(
                    GarminErrorCode::InvalidCredentials,
                    "credential_login",
                    Some(status),
                ));
            }
            _ if login.is_rate_limited() => {
                return Err(GarminError::provider(
                    GarminErrorCode::RateLimited,
                    "credential_login",
                    Some(status),
                ));
            }
            _ => {
                return Err(GarminError::provider(
                    GarminErrorCode::SsoContractError,
                    "credential_login",
                    Some(status),
                ));
            }
        };

        let tokens = self.exchange_service_ticket(&ticket).await?;
        Ok(AuthenticationOutcome::Authenticated(tokens))
    }

    async fn verify_mfa(&self, code: &str, method: &str) -> Result<String, GarminError> {
        let mfa_url = self
            .endpoints
            .sso
            .join("mobile/api/mfa/verifyCode")
            .map_err(|_| {
                GarminError::provider(GarminErrorCode::SsoContractError, "mfa_login", None)
            })?;
        let response = self
            .http
            .post(mfa_url)
            .query(&[
                ("clientId", IOS_CLIENT_ID),
                ("locale", "en-US"),
                ("service", self.endpoints.service_url.as_str()),
            ])
            .headers(self.mobile_headers())
            .json(&MfaRequest {
                mfa_method: method,
                mfa_verification_code: code,
                remember_my_browser: true,
                reconsent_list: Vec::new(),
                mfa_setup: false,
            })
            .send()
            .await
            .map_err(|error| GarminError::transport("mfa_login", &error))?;
        let status = response.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(GarminError::provider(
                GarminErrorCode::RateLimited,
                "mfa_login",
                Some(status),
            ));
        }
        let result = response.json::<SsoResponse>().await.map_err(|_| {
            GarminError::provider(GarminErrorCode::SsoContractError, "mfa_login", Some(status))
        })?;
        if result.is_rate_limited() {
            return Err(GarminError::provider(
                GarminErrorCode::RateLimited,
                "mfa_login",
                Some(status),
            ));
        }
        if result.response_type() != Some("SUCCESSFUL") {
            return Err(GarminError::provider(
                GarminErrorCode::InvalidMfa,
                "mfa_login",
                Some(status),
            ));
        }
        result.service_ticket_id.ok_or_else(|| {
            GarminError::provider(GarminErrorCode::SsoContractError, "mfa_login", Some(status))
        })
    }

    async fn exchange_service_ticket(&self, ticket: &str) -> Result<GarminTokens, GarminError> {
        for client_id in DI_CLIENT_IDS {
            let authorization = format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(format!("{client_id}:"))
            );
            let response = self
                .http
                .post(self.endpoints.di_token.clone())
                .headers(self.native_headers())
                .header(AUTHORIZATION, authorization)
                .form(&[
                    ("client_id", *client_id),
                    ("service_ticket", ticket),
                    ("grant_type", DI_GRANT_TYPE),
                    ("service_url", self.endpoints.service_url.as_str()),
                ])
                .send()
                .await
                .map_err(|error| GarminError::transport("token_exchange", &error))?;
            let status = response.status();
            if status == StatusCode::TOO_MANY_REQUESTS {
                return Err(GarminError::provider(
                    GarminErrorCode::RateLimited,
                    "token_exchange",
                    Some(status),
                ));
            }
            if !status.is_success() {
                continue;
            }
            let token = response.json::<TokenResponse>().await.map_err(|_| {
                GarminError::provider(
                    GarminErrorCode::SsoContractError,
                    "token_exchange",
                    Some(status),
                )
            })?;
            if token.access_token.trim().is_empty() {
                continue;
            }
            let resolved_client_id =
                jwt_client_id(&token.access_token).unwrap_or_else(|| (*client_id).to_owned());
            return Ok(GarminTokens {
                di_token: token.access_token,
                di_refresh_token: token.refresh_token,
                di_client_id: Some(resolved_client_id),
            });
        }
        Err(GarminError::provider(
            GarminErrorCode::InvalidCredentials,
            "token_exchange",
            None,
        ))
    }

    pub async fn refresh(&self, tokens: &GarminTokens) -> Result<GarminTokens, GarminError> {
        let refresh_token = tokens.di_refresh_token.as_deref().ok_or_else(|| {
            GarminError::provider(GarminErrorCode::InvalidCredentials, "token_refresh", None)
        })?;
        let client_id = tokens.di_client_id.as_deref().ok_or_else(|| {
            GarminError::provider(GarminErrorCode::InvalidCredentials, "token_refresh", None)
        })?;
        let authorization = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{client_id}:"))
        );
        let response = self
            .http
            .post(self.endpoints.di_token.clone())
            .headers(self.native_headers())
            .header(AUTHORIZATION, authorization)
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", client_id),
                ("refresh_token", refresh_token),
            ])
            .send()
            .await
            .map_err(|error| GarminError::transport("token_refresh", &error))?;
        let status = response.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(GarminError::provider(
                GarminErrorCode::RateLimited,
                "token_refresh",
                Some(status),
            ));
        }
        if !status.is_success() {
            return Err(GarminError::provider(
                GarminErrorCode::InvalidCredentials,
                "token_refresh",
                Some(status),
            ));
        }
        let token = response.json::<TokenResponse>().await.map_err(|_| {
            GarminError::provider(
                GarminErrorCode::SsoContractError,
                "token_refresh",
                Some(status),
            )
        })?;
        if token.access_token.trim().is_empty() {
            return Err(GarminError::provider(
                GarminErrorCode::SsoContractError,
                "token_refresh",
                Some(status),
            ));
        }
        Ok(GarminTokens {
            di_client_id: jwt_client_id(&token.access_token)
                .or_else(|| tokens.di_client_id.clone()),
            di_refresh_token: token
                .refresh_token
                .or_else(|| tokens.di_refresh_token.clone()),
            di_token: token.access_token,
        })
    }

    pub async fn validate(&self, tokens: &GarminTokens) -> Result<GarminTokens, GarminError> {
        let mut active = tokens.clone();
        self.authenticated_get(
            &mut active,
            "activitylist-service/activities/search/activities",
            &[("start", "0".to_owned()), ("limit", "1".to_owned())],
            "token_restore",
        )
        .await?;
        Ok(active)
    }

    pub(crate) async fn authenticated_get(
        &self,
        tokens: &mut GarminTokens,
        path: &str,
        query: &[(&str, String)],
        phase: &'static str,
    ) -> Result<reqwest::Response, GarminError> {
        if Self::token_expires_within(tokens, Duration::from_secs(300)) {
            *tokens = self.refresh(tokens).await?;
        }
        let url = self.api_url(path)?;
        let mut response = self
            .http
            .get(url.clone())
            .headers(self.native_headers())
            .bearer_auth(&tokens.di_token)
            .query(query)
            .send()
            .await
            .map_err(|error| GarminError::transport(phase, &error))?;
        if response.status() == StatusCode::UNAUTHORIZED {
            *tokens = self.refresh(tokens).await?;
            response = self
                .http
                .get(url)
                .headers(self.native_headers())
                .bearer_auth(&tokens.di_token)
                .query(query)
                .send()
                .await
                .map_err(|error| GarminError::transport(phase, &error))?;
        }
        match response.status() {
            status if status.is_success() => Ok(response),
            StatusCode::TOO_MANY_REQUESTS => Err(GarminError::provider(
                GarminErrorCode::RateLimited,
                phase,
                Some(response.status()),
            )),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(GarminError::provider(
                GarminErrorCode::InvalidCredentials,
                phase,
                Some(response.status()),
            )),
            status if status.is_server_error() => Err(GarminError::provider(
                GarminErrorCode::NetworkError,
                phase,
                Some(status),
            )),
            status => Err(GarminError::provider(
                GarminErrorCode::SsoContractError,
                phase,
                Some(status),
            )),
        }
    }

    pub fn token_expires_within(tokens: &GarminTokens, duration: Duration) -> bool {
        let Some(expiry) = jwt_expiry(&tokens.di_token) else {
            return false;
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        expiry <= now.saturating_add(duration.as_secs())
    }

    pub(crate) fn api_url(&self, path: &str) -> Result<Url, GarminError> {
        self.endpoints.connect_api.join(path).map_err(|_| {
            GarminError::provider(GarminErrorCode::SsoContractError, "api_request", None)
        })
    }

    pub(crate) fn native_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            USER_AGENT,
            NATIVE_USER_AGENT.parse().expect("static header"),
        );
        headers.insert(
            "x-garmin-user-agent",
            NATIVE_X_GARMIN_USER_AGENT.parse().expect("static header"),
        );
        headers.insert(ACCEPT, "application/json".parse().expect("static header"));
        headers.insert(CACHE_CONTROL, "no-cache".parse().expect("static header"));
        headers
    }

    fn mobile_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(USER_AGENT, IOS_USER_AGENT.parse().expect("static header"));
        headers.insert(
            ACCEPT,
            "application/json, text/plain, */*"
                .parse()
                .expect("static header"),
        );
        headers.insert(
            CONTENT_TYPE,
            "application/json".parse().expect("static header"),
        );
        headers.insert(
            ORIGIN,
            self.endpoints
                .sso
                .origin()
                .ascii_serialization()
                .parse()
                .expect("validated origin"),
        );
        headers
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoginRequest<'a> {
    username: &'a str,
    password: &'a str,
    remember_me: bool,
    captcha_token: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MfaRequest<'a> {
    mfa_method: &'a str,
    mfa_verification_code: &'a str,
    remember_my_browser: bool,
    reconsent_list: Vec<String>,
    mfa_setup: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SsoResponse {
    #[serde(default)]
    response_status: Option<ResponseStatus>,
    #[serde(default)]
    service_ticket_id: Option<String>,
    #[serde(default)]
    customer_mfa_info: Option<CustomerMfaInfo>,
    #[serde(default)]
    error: Option<HashMap<String, Value>>,
}

impl SsoResponse {
    fn response_type(&self) -> Option<&str> {
        self.response_status.as_ref()?.kind.as_deref()
    }

    fn mfa_method(&self) -> &str {
        self.customer_mfa_info
            .as_ref()
            .and_then(|info| info.mfa_last_method_used.as_deref())
            .filter(|method| matches!(*method, "email" | "sms"))
            .unwrap_or("email")
    }

    fn is_rate_limited(&self) -> bool {
        self.error
            .as_ref()
            .and_then(|error| error.get("status-code"))
            .and_then(Value::as_str)
            == Some("429")
    }
}

#[derive(Debug, Deserialize)]
struct ResponseStatus {
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CustomerMfaInfo {
    mfa_last_method_used: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}

fn jwt_payload(token: &str) -> Option<Value> {
    let mut segments = token.split('.');
    let header = segments.next()?;
    let payload = segments.next()?;
    let signature = segments.next()?;
    if segments.next().is_some() || header.is_empty() || payload.is_empty() || signature.is_empty()
    {
        return None;
    }
    let header: Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(header)
            .ok()?,
    )
    .ok()?;
    let algorithm = header.get("alg")?.as_str()?;
    if algorithm.is_empty() || algorithm.eq_ignore_ascii_case("none") {
        return None;
    }
    serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .ok()?,
    )
    .ok()
}

fn jwt_client_id(token: &str) -> Option<String> {
    jwt_payload(token)?
        .get("client_id")?
        .as_str()
        .filter(|value| !value.is_empty() && value.len() <= 200)
        .map(str::to_owned)
}

fn jwt_expiry(token: &str) -> Option<u64> {
    let value = jwt_payload(token)?.get("exp")?.clone();
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse::<u64>().ok(),
        _ => None,
    }
    .filter(|value| *value > 0)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::extract::{Form, Query, State};
    use axum::http::HeaderMap;
    use axum::response::IntoResponse;
    use axum::routing::{get, post};
    use axum::{Json, Router};

    use super::*;

    #[derive(Clone, Default)]
    struct FakeState {
        mfa_calls: Arc<Mutex<usize>>,
        exchange_clients: Arc<Mutex<Vec<String>>>,
    }

    async fn login(
        Query(query): Query<HashMap<String, String>>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> impl IntoResponse {
        assert_eq!(
            query.get("clientId").map(String::as_str),
            Some(IOS_CLIENT_ID)
        );
        assert_eq!(query.get("locale").map(String::as_str), Some("en-US"));
        assert_eq!(
            body.get("username").and_then(Value::as_str),
            Some("user@example.invalid")
        );
        assert_eq!(body.get("password").and_then(Value::as_str), Some("secret"));
        assert_eq!(body.get("rememberMe").and_then(Value::as_bool), Some(true));
        assert!(headers.get(USER_AGENT).is_some());
        (
            [("set-cookie", "GARMIN-SSO=challenge; Path=/; HttpOnly")],
            Json(serde_json::json!({
                "responseStatus": {"type": "MFA_REQUIRED"},
                "customerMfaInfo": {"mfaLastMethodUsed": "email"}
            })),
        )
    }

    async fn mfa(
        State(state): State<FakeState>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        assert!(
            headers
                .get("cookie")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.contains("GARMIN-SSO=challenge"))
        );
        assert_eq!(
            body.get("mfaVerificationCode").and_then(Value::as_str),
            Some("123456")
        );
        *state.mfa_calls.lock().unwrap() += 1;
        Json(serde_json::json!({
            "responseStatus": {"type": "SUCCESSFUL"},
            "serviceTicketId": "service-ticket"
        }))
    }

    async fn token(
        State(state): State<FakeState>,
        headers: HeaderMap,
        Form(form): Form<HashMap<String, String>>,
    ) -> impl IntoResponse {
        if form.get("grant_type").map(String::as_str) == Some("refresh_token") {
            assert_eq!(
                form.get("refresh_token").map(String::as_str),
                Some("refresh-one")
            );
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "access_token": jwt("native-client", 4_102_444_800_u64),
                    "refresh_token": "refresh-two"
                })),
            );
        }
        assert_eq!(
            form.get("service_ticket").map(String::as_str),
            Some("service-ticket")
        );
        assert_eq!(
            form.get("grant_type").map(String::as_str),
            Some(DI_GRANT_TYPE)
        );
        let client_id = form.get("client_id").cloned().unwrap();
        state
            .exchange_clients
            .lock()
            .unwrap()
            .push(client_id.clone());
        assert!(headers.get(AUTHORIZATION).is_some());
        if client_id == DI_CLIENT_IDS[0] {
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({})));
        }
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "access_token": jwt("native-client", 4_102_444_800_u64),
                "refresh_token": "refresh-one"
            })),
        )
    }

    async fn validate(
        headers: HeaderMap,
        Query(query): Query<HashMap<String, String>>,
    ) -> Json<Value> {
        assert_eq!(query.get("start").map(String::as_str), Some("0"));
        assert_eq!(query.get("limit").map(String::as_str), Some("1"));
        assert!(
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("Bearer "))
        );
        Json(serde_json::json!([]))
    }

    fn jwt(client_id: &str, expiry: u64) -> String {
        let encode = |value: Value| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(serde_json::to_vec(&value).unwrap())
        };
        format!(
            "{}.{}.signature",
            encode(serde_json::json!({"alg": "RS256"})),
            encode(serde_json::json!({"client_id": client_id, "exp": expiry}))
        )
    }

    async fn fake_client() -> (GarminClient, FakeState) {
        let state = FakeState::default();
        let app = Router::new()
            .route("/mobile/api/login", post(login))
            .route("/mobile/api/mfa/verifyCode", post(mfa))
            .route("/di-oauth2-service/oauth/token", post(token))
            .route(
                "/activitylist-service/activities/search/activities",
                get(validate),
            )
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let root = Url::parse(&format!("http://{address}/")).unwrap();
        let endpoints = Endpoints {
            sso: root.clone(),
            di_token: root.join("di-oauth2-service/oauth/token").unwrap(),
            connect_api: root.clone(),
            service_url: root.join("gcm/ios").unwrap().to_string(),
        };
        (GarminClient::new(endpoints).unwrap(), state)
    }

    #[tokio::test]
    async fn fake_service_exercises_login_mfa_exchange_and_refresh() {
        let (client, state) = fake_client().await;
        assert_eq!(
            client
                .authenticate("user@example.invalid", "secret", None)
                .await
                .unwrap(),
            AuthenticationOutcome::MfaRequired
        );
        let AuthenticationOutcome::Authenticated(tokens) = client
            .authenticate("user@example.invalid", "secret", Some("123456"))
            .await
            .unwrap()
        else {
            panic!("expected authenticated tokens");
        };
        assert_eq!(tokens.di_refresh_token.as_deref(), Some("refresh-one"));
        assert_eq!(tokens.di_client_id.as_deref(), Some("native-client"));
        assert_eq!(*state.mfa_calls.lock().unwrap(), 1);
        assert_eq!(
            state.exchange_clients.lock().unwrap().as_slice(),
            &DI_CLIENT_IDS[..2]
        );

        let refreshed = client.refresh(&tokens).await.unwrap();
        assert_eq!(refreshed.di_refresh_token.as_deref(), Some("refresh-two"));
        assert!(!GarminClient::token_expires_within(
            &refreshed,
            Duration::from_secs(300)
        ));
        assert_eq!(client.validate(&refreshed).await.unwrap(), refreshed);
    }

    #[test]
    fn malformed_or_unsigned_jwt_never_drives_expiry_or_client_id() {
        let unsigned = format!(
            "{}.{}.signature",
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(serde_json::to_vec(&serde_json::json!({"alg": "none"})).unwrap()),
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&serde_json::json!({"client_id": "native-client", "exp": 1}))
                    .unwrap()
            )
        );
        for token in ["not-a-jwt", "e30.e30.", &unsigned] {
            assert_eq!(jwt_expiry(token), None);
            assert_eq!(jwt_client_id(token), None);
        }
    }

    #[test]
    fn production_endpoints_are_strictly_allowlisted() {
        assert!(Endpoints::production().validate_production().is_ok());
        let mut endpoints = Endpoints::production();
        endpoints.sso = Url::parse("https://sso.garmin.cn.attacker.invalid/").unwrap();
        assert!(endpoints.validate_production().is_err());
    }
}
