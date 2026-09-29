mod auth;
mod config;
mod gen_auth;
mod oidc;
mod status;

use std::{
    collections::{BTreeMap, HashMap},
    convert::Infallible,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{ConnectInfo, Form, Query, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{StreamExt, future::join_all, stream::FuturesUnordered};
use minijinja::{Environment, value::Value as TemplateValue};
use serde::Deserialize;
use serde_json::{Map, json};
use subtle::ConstantTimeEq;
use tower_http::services::ServeDir;
use tracing::{error, info, warn};

use crate::{
    auth::{
        LoginLimiter, SessionData, attach_session_cookie, clear_session_cookie, csrf_matches,
        is_safe_next_url, is_valid_password, load_session, new_token, now_seconds,
    },
    config::{Settings, load_settings},
    oidc::OidcProvider,
    status::SystemMonitor,
};

const LOGIN_TEMPLATE: &str = include_str!("../../templates/login.html");
const INDEX_TEMPLATE: &str = include_str!("../../templates/index.html");
const OIDC_LOGIN_TEMPLATE_CALL: &str =
    "{{ url_for('auth.oidc_login', next=request.args.get('next', '')) }}";

#[derive(Clone)]
struct AppState {
    settings: Arc<Settings>,
    limiter: Arc<LoginLimiter>,
    monitor: Arc<SystemMonitor>,
    oidc: Option<Arc<OidcProvider>>,
    templates: Arc<Environment<'static>>,
    probe_http: reqwest::Client,
}

#[derive(Deserialize, Default)]
struct LoginForm {
    #[serde(default)]
    csrf_token: String,
    #[serde(default)]
    website: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
}

#[derive(Deserialize, Default)]
struct LogoutForm {
    #[serde(default)]
    csrf_token: String,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("gen-auth") => {
            gen_auth::run_or_exit();
            return;
        }
        Some("serve") | None => {}
        Some(_) => {
            eprintln!("usage: starry-cloud [serve|gen-auth]");
            std::process::exit(2);
        }
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    if let Err(error) = run().await {
        eprintln!("starry-cloud: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let settings = Arc::new(load_settings()?);
    let templates = Arc::new(load_templates()?);
    let oidc = if settings.auth.oidc_enabled {
        Some(Arc::new(OidcProvider::new()?))
    } else {
        None
    };
    let probe_http = reqwest::Client::builder()
        .user_agent(concat!(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) ",
            "AppleWebKit/537.36 (KHTML, like Gecko) ",
            "Chrome/137.0.0.0 Safari/537.36"
        ))
        .build()
        .map_err(|error| format!("could not configure service probes: {error}"))?;

    let state = AppState {
        settings,
        limiter: Arc::new(LoginLimiter::default()),
        monitor: Arc::new(SystemMonitor::default()),
        oidc,
        templates,
        probe_http,
    };

    let app = Router::new()
        .route("/", get(index).head(index))
        .route("/robots.txt", get(robots_txt))
        .route("/health", get(health))
        .route("/login", get(login_get).post(login_post))
        .route("/login/oidc", get(oidc_login))
        .route("/login/oidc/callback", get(oidc_callback))
        .route("/logout", post(logout))
        .route("/api/service-status", get(service_status))
        .route("/api/system-stats", get(system_stats))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state.clone())
        .layer(middleware::from_fn(add_security_headers))
        .layer(middleware::from_fn_with_state(state, add_access_log));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:5000")
        .await
        .map_err(|error| format!("failed to bind to port 5000: {error}"))?;
    info!("Starry Cloud Rust server listening on 0.0.0.0:5000");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .map_err(|error| format!("HTTP server failed: {error}"))
}

fn load_templates() -> Result<Environment<'static>, String> {
    let mut env = Environment::new();
    if !LOGIN_TEMPLATE.contains("{{ csrf_token() }}")
        || !LOGIN_TEMPLATE.contains(OIDC_LOGIN_TEMPLATE_CALL)
        || !INDEX_TEMPLATE.contains("{{ csrf_token() }}")
    {
        return Err("template helpers changed; update their Rust rendering context".to_owned());
    }
    let login = LOGIN_TEMPLATE
        .replace("{{ csrf_token() }}", "{{ csrf_token }}")
        .replace(OIDC_LOGIN_TEMPLATE_CALL, "{{ oidc_login_url }}");
    let index = INDEX_TEMPLATE.replace("{{ csrf_token() }}", "{{ csrf_token }}");
    env.add_template_owned("login.html", login)
        .map_err(|error| format!("invalid login template: {error}"))?;
    env.add_template_owned("index.html", index)
        .map_err(|error| format!("invalid dashboard template: {error}"))?;
    Ok(env)
}

async fn add_security_headers(request: Request, next: Next) -> Response {
    let is_static = request.uri().path().starts_with("/static/");
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    set_default_header(
        headers,
        "x-robots-tag",
        "noindex, nofollow, noarchive, nosnippet, noimageindex",
    );
    set_default_header(headers, "x-content-type-options", "nosniff");
    set_default_header(headers, "referrer-policy", "no-referrer");
    set_default_header(
        headers,
        "content-security-policy",
        "frame-ancestors 'none'; base-uri 'self'; object-src 'none'",
    );
    set_default_header(headers, "x-frame-options", "DENY");
    if !is_static {
        set_default_header(
            headers,
            "cache-control",
            "no-store, no-cache, must-revalidate, max-age=0",
        );
        set_default_header(headers, "pragma", "no-cache");
    }
    response
}

async fn add_access_log(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let forwarded_for = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-")
        .to_owned();
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| *address)
        .unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], 0)));
    let client = peer_ip(
        request.headers(),
        peer,
        state.settings.app.trusted_proxy_hops,
    );
    let started = Instant::now();
    let response = next.run(request).await;
    info!(
        "ip={} xff={} method={} path={} status={} rt={:.3}",
        client,
        forwarded_for,
        method,
        path,
        response.status().as_u16(),
        started.elapsed().as_secs_f64()
    );
    response
}

fn set_default_header(headers: &mut HeaderMap, name: &'static str, value: &'static str) {
    let name = header::HeaderName::from_static(name);
    if !headers.contains_key(&name) {
        headers.insert(name, HeaderValue::from_static(value));
    }
}

fn html_response(body: String) -> Response {
    let mut response = body.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}

fn text_response(status: StatusCode, body: &'static str) -> Response {
    (status, body).into_response()
}

fn redirect_response(location: &str) -> Response {
    let mut response = StatusCode::FOUND.into_response();
    if let Ok(value) = HeaderValue::from_str(&normalize_redirect(location)) {
        response.headers_mut().insert(header::LOCATION, value);
    }
    response
}

fn normalize_redirect(location: &str) -> String {
    if let Ok(url) = url::Url::parse(location) {
        return if matches!(url.scheme(), "http" | "https") {
            location.to_owned()
        } else {
            "/".to_owned()
        };
    }
    if !location.starts_with('/') || location.starts_with("//") {
        return "/".to_owned();
    }
    let Ok(base) = url::Url::parse("http://localhost") else {
        return "/".to_owned();
    };
    let Ok(url) = base.join(location) else {
        return "/".to_owned();
    };
    let mut target = url.path().to_owned();
    if let Some(query) = url.query() {
        target.push('?');
        target.push_str(query);
    }
    if let Some(fragment) = url.fragment() {
        target.push('#');
        target.push_str(fragment);
    }
    target
}

fn login_url(next: Option<&str>) -> String {
    match next {
        Some(next) if !next.is_empty() => format!("/login?{}", query_pair("next", next)),
        _ => "/login".to_owned(),
    }
}

fn query_pair(name: &str, value: &str) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    serializer.append_pair(name, value);
    serializer.finish()
}

fn oidc_login_url(next: &str) -> String {
    format!("/login/oidc?{}", query_pair("next", next))
}

fn set_no_cache(response: &mut Response) {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate, max-age=0"),
    );
    response
        .headers_mut()
        .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
}

fn with_session(mut response: Response, mut session: SessionData, state: &AppState) -> Response {
    if session.authenticated {
        session.expires_at =
            Some(now_seconds().saturating_add(state.settings.auth.session_days as u64 * 86_400));
    }
    if let Err(error) =
        attach_session_cookie(response.headers_mut(), &session, &state.settings.auth)
    {
        warn!("could not issue session cookie: {error}");
    }
    response
}

fn get_session(headers: &HeaderMap, state: &AppState) -> SessionData {
    load_session(headers, &state.settings.auth.secret_key)
}

fn peer_ip(headers: &HeaderMap, peer: SocketAddr, trusted_hops: usize) -> String {
    if trusted_hops > 0 {
        if let Some(value) = forwarded_header(headers, "x-forwarded-for", trusted_hops) {
            return value;
        }
    }
    peer.ip().to_string()
}

fn forwarded_header(
    headers: &HeaderMap,
    name: &'static str,
    trusted_hops: usize,
) -> Option<String> {
    let value = headers.get(name)?.to_str().ok()?;
    let values = value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    if values.len() < trusted_hops {
        return None;
    }
    values
        .get(values.len().checked_sub(trusted_hops)?)
        .map(|item| (*item).to_owned())
}

fn external_url(headers: &HeaderMap, peer: SocketAddr, state: &AppState, path: &str) -> String {
    let trusted_hops = state.settings.app.trusted_proxy_hops;
    let proto = if trusted_hops > 0 {
        forwarded_header(headers, "x-forwarded-proto", trusted_hops)
    } else {
        None
    }
    .unwrap_or_else(|| "http".to_owned());
    let host = if trusted_hops > 0 {
        forwarded_header(headers, "x-forwarded-host", trusted_hops)
    } else {
        None
    }
    .or_else(|| {
        headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    })
    .unwrap_or_else(|| peer.to_string());
    format!("{proto}://{host}{path}")
}

fn require_login(
    headers: &HeaderMap,
    uri: &Uri,
    peer: SocketAddr,
    state: &AppState,
) -> Result<SessionData, Response> {
    let session = get_session(headers, state);
    if !session.authenticated {
        return Err(redirect_response(&login_url(Some(uri.path()))));
    }
    let _client_ip = peer_ip(headers, peer, state.settings.app.trusted_proxy_hops);
    Ok(session)
}

async fn login_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let mut session = get_session(&headers, &state);
    if session.csrf_token.is_empty() {
        session.csrf_token = new_token();
    }
    let error = session.flash_message.take();
    let next = query.get("next").map(String::as_str).unwrap_or_default();
    let mut context = BTreeMap::new();
    context.insert(
        "cfg".to_owned(),
        TemplateValue::from_serialize(&state.settings.app),
    );
    context.insert("error".to_owned(), TemplateValue::from_serialize(&error));
    context.insert(
        "password_enabled".to_owned(),
        TemplateValue::from(state.settings.auth.password_enabled),
    );
    context.insert(
        "oidc_enabled".to_owned(),
        TemplateValue::from(state.settings.auth.oidc_enabled),
    );
    context.insert(
        "oidc_provider_name".to_owned(),
        TemplateValue::from(state.settings.auth.oidc_provider_name.clone()),
    );
    context.insert(
        "oidc_login_url".to_owned(),
        TemplateValue::from(oidc_login_url(next)),
    );
    context.insert(
        "csrf_token".to_owned(),
        TemplateValue::from(session.csrf_token.clone()),
    );
    let response = match state
        .templates
        .get_template("login.html")
        .and_then(|template| template.render(context))
    {
        Ok(html) => html_response(html),
        Err(error) => {
            error!("login template render failed: {error}");
            text_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error")
        }
    };
    with_session(response, session, &state)
}

async fn login_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(query): Query<HashMap<String, String>>,
    Form(form): Form<LoginForm>,
) -> Response {
    let auth = &state.settings.auth;
    if !auth.password_enabled {
        return text_response(StatusCode::NOT_FOUND, "Password authentication is disabled");
    }

    let mut session = get_session(&headers, &state);
    let client_ip = peer_ip(&headers, peer, state.settings.app.trusted_proxy_hops);
    let next = query.get("next").map(String::as_str).unwrap_or_default();
    let redirect_to = login_url((!next.is_empty()).then_some(next));

    if !csrf_matches(&session, &form.csrf_token) {
        session.flash_message = Some("Invalid or missing security token.".to_owned());
        return with_session(redirect_response(&redirect_to), session, &state);
    }
    let remaining = state.limiter.remaining_seconds(&client_ip, auth);
    if remaining > 0 {
        session.flash_message = Some(lockout_message(remaining));
        return with_session(redirect_response(&redirect_to), session, &state);
    }
    if !form.website.trim().is_empty() {
        state.limiter.register_failure(&client_ip, auth);
        let remaining = state.limiter.remaining_seconds(&client_ip, auth);
        session.flash_message = Some(if remaining > 0 {
            lockout_message(remaining)
        } else {
            "Invalid username or password".to_owned()
        });
        return with_session(redirect_response(&redirect_to), session, &state);
    }

    let username = auth.username.as_deref().unwrap_or_default();
    let password_hash = auth.password_hash.as_deref().unwrap_or_default();
    let username_matches = bool::from(username.as_bytes().ct_eq(form.username.as_bytes()));
    let password_matches = is_valid_password(password_hash, &form.password);

    if username_matches && password_matches {
        state.limiter.clear(&client_ip);
        session = SessionData {
            authenticated: true,
            csrf_token: new_token(),
            expires_at: Some(now_seconds().saturating_add(auth.session_days as u64 * 86_400)),
            ..SessionData::default()
        };
        let target = if is_safe_next_url(next) { next } else { "/" };
        return with_session(redirect_response(target), session, &state);
    }

    state.limiter.register_failure(&client_ip, auth);
    let remaining = state.limiter.remaining_seconds(&client_ip, auth);
    session.flash_message = Some(if remaining > 0 {
        lockout_message(remaining)
    } else {
        "Invalid username or password".to_owned()
    });
    with_session(redirect_response(&redirect_to), session, &state)
}

fn lockout_message(remaining_seconds: u64) -> String {
    let minutes = remaining_seconds.div_ceil(60).max(1);
    format!("Too many failed attempts. Try again in {minutes} minute(s).")
}

async fn oidc_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let auth = &state.settings.auth;
    let Some(oidc) = state.oidc.as_ref() else {
        return text_response(StatusCode::NOT_FOUND, "OIDC authentication is disabled");
    };
    let mut session = get_session(&headers, &state);
    let next = query.get("next").map(String::as_str).unwrap_or_default();
    session.oidc_next = Some(if is_safe_next_url(next) {
        next.to_owned()
    } else {
        String::new()
    });
    let callback = external_url(&headers, peer, &state, "/login/oidc/callback");
    match oidc.authorization(auth, &callback).await {
        Ok(authorization) => {
            session.oidc_state = Some(authorization.state);
            session.oidc_nonce = Some(authorization.nonce);
            session.oidc_verifier = Some(authorization.verifier);
            with_session(redirect_response(&authorization.url), session, &state)
        }
        Err(error) => {
            error!("could not start OIDC login: {error}");
            session.flash_message = Some("OIDC sign-in failed. Please try again.".to_owned());
            with_session(redirect_response("/login"), session, &state)
        }
    }
}

async fn oidc_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let auth = &state.settings.auth;
    let Some(oidc) = state.oidc.as_ref() else {
        return text_response(StatusCode::NOT_FOUND, "OIDC authentication is disabled");
    };
    let mut session = get_session(&headers, &state);
    if query.contains_key("error") {
        return oidc_failure(session, &state, "OIDC sign-in failed. Please try again.");
    }

    let Some(returned_state) = query.get("state") else {
        return oidc_failure(session, &state, "OIDC sign-in failed. Please try again.");
    };
    let Some(expected_state) = session.oidc_state.take() else {
        return oidc_failure(session, &state, "OIDC sign-in failed. Please try again.");
    };
    if !bool::from(expected_state.as_bytes().ct_eq(returned_state.as_bytes())) {
        return oidc_failure(session, &state, "OIDC sign-in failed. Please try again.");
    }
    let (Some(code), Some(nonce), Some(verifier)) = (
        query.get("code"),
        session.oidc_nonce.take(),
        session.oidc_verifier.take(),
    ) else {
        return oidc_failure(session, &state, "OIDC sign-in failed. Please try again.");
    };
    let callback = external_url(&headers, peer, &state, "/login/oidc/callback");
    let identity = match oidc
        .identity(auth, &callback, code, &nonce, &verifier)
        .await
    {
        Ok(identity) => identity,
        Err(error) => {
            warn!("OIDC token exchange or state validation failed ({})", error);
            return oidc_failure(session, &state, "OIDC sign-in failed. Please try again.");
        }
    };

    if identity.subject.is_empty() {
        return oidc_failure(
            session,
            &state,
            "The identity provider did not return a valid user identity.",
        );
    }
    if !auth.oidc_allowed_emails.is_empty() {
        let email = identity
            .email
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        if !identity.email_verified || !auth.oidc_allowed_emails.contains(&email) {
            warn!(
                email_matched = auth.oidc_allowed_emails.contains(&email),
                email_verified = identity.email_verified,
                "OIDC access denied by email allowlist"
            );
            return oidc_failure(session, &state, "This account is not allowed to sign in.");
        }
    }

    let next = session.oidc_next.take().unwrap_or_default();
    session = SessionData {
        authenticated: true,
        csrf_token: new_token(),
        expires_at: Some(now_seconds().saturating_add(auth.session_days as u64 * 86_400)),
        ..SessionData::default()
    };
    let target = if is_safe_next_url(&next) {
        next.as_str()
    } else {
        "/"
    };
    with_session(redirect_response(target), session, &state)
}

fn oidc_failure(mut session: SessionData, state: &AppState, message: &str) -> Response {
    session.oidc_state = None;
    session.oidc_nonce = None;
    session.oidc_verifier = None;
    session.oidc_next = None;
    session.flash_message = Some(message.to_owned());
    with_session(redirect_response("/login"), session, state)
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LogoutForm>,
) -> Response {
    let session = get_session(&headers, &state);
    if !csrf_matches(&session, &form.csrf_token) {
        return text_response(StatusCode::BAD_REQUEST, "");
    }
    let mut response = redirect_response("/login");
    clear_session_cookie(response.headers_mut(), state.settings.auth.secure_cookie);
    response
}

async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    uri: Uri,
) -> Response {
    let mut session = match require_login(&headers, &uri, peer, &state) {
        Ok(session) => session,
        Err(response) => return response,
    };
    if session.csrf_token.is_empty() {
        session.csrf_token = new_token();
    }
    let mut context = BTreeMap::new();
    context.insert(
        "cfg".to_owned(),
        TemplateValue::from_serialize(&state.settings.app),
    );
    context.insert(
        "services".to_owned(),
        TemplateValue::from_serialize(&state.settings.app.services),
    );
    context.insert(
        "system_hostname".to_owned(),
        TemplateValue::from(state.monitor.hostname()),
    );
    context.insert(
        "device_model".to_owned(),
        TemplateValue::from(state.monitor.device_model()),
    );
    context.insert(
        "csrf_token".to_owned(),
        TemplateValue::from(session.csrf_token.clone()),
    );
    let response = match state
        .templates
        .get_template("index.html")
        .and_then(|template| template.render(context))
    {
        Ok(html) => html_response(html),
        Err(error) => {
            error!("dashboard template render failed: {error}");
            text_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error")
        }
    };
    with_session(response, session, &state)
}

async fn robots_txt() -> Response {
    let mut response = "User-agent: *\nDisallow: /\n".into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

async fn health() -> &'static str {
    "ok"
}

async fn system_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    uri: Uri,
) -> Response {
    let session = match require_login(&headers, &uri, peer, &state) {
        Ok(session) => session,
        Err(response) => return response,
    };
    let mut response = Json(state.monitor.stats()).into_response();
    set_no_cache(&mut response);
    with_session(response, session, &state)
}

async fn service_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    uri: Uri,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let session = match require_login(&headers, &uri, peer, &state) {
        Ok(session) => session,
        Err(response) => return response,
    };
    let services = state.settings.app.services.clone();

    if query.get("stream").is_some_and(|value| value == "1") {
        let pending = FuturesUnordered::new();
        for (index, service) in services.into_iter().enumerate() {
            let client = state.probe_http.clone();
            pending.push(async move {
                let online = check_service(&client, &service).await;
                (index, online)
            });
        }
        let stream = futures_util::stream::unfold(pending, |mut pending| async move {
            let (index, online) = pending.next().await?;
            let record = json!({"id": index.to_string(), "online": online});
            let line = Bytes::from(format!("{record}\n"));
            Some((Ok::<_, Infallible>(line), pending))
        });
        let mut response = Response::new(Body::from_stream(stream));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-ndjson"),
        );
        response.headers_mut().insert(
            HeaderName::from_static("x-accel-buffering"),
            HeaderValue::from_static("no"),
        );
        set_no_cache(&mut response);
        return with_session(response, session, &state);
    }

    let checks = join_all(services.iter().enumerate().map(|(index, service)| {
        let client = state.probe_http.clone();
        async move { (index, check_service(&client, service).await) }
    }))
    .await;
    let mut statuses = Map::new();
    for (index, online) in checks {
        statuses.insert(index.to_string(), json!(online));
    }
    let mut response = Json(json!({"statuses": statuses})).into_response();
    set_no_cache(&mut response);
    with_session(response, session, &state)
}

async fn check_service(client: &reqwest::Client, service: &config::Service) -> bool {
    let mut urls = Vec::new();
    if let Some(url) = service.web_url.as_deref() {
        urls.push(url);
    }
    if let Some(url) = service.lan_url.as_deref() {
        urls.push(url);
    }
    for url in urls {
        if service_online(client, url).await {
            return true;
        }
    }
    false
}

async fn service_online(client: &reqwest::Client, target: &str) -> bool {
    let Ok(url) = url::Url::parse(target) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return false;
    }
    let status = match client
        .request(Method::HEAD, target)
        .timeout(Duration::from_millis(2500))
        .send()
        .await
    {
        Ok(response) => response.status().as_u16(),
        Err(error) => match error.status() {
            Some(status) => status.as_u16(),
            None => return false,
        },
    };
    if !matches!(status, 401 | 403 | 405 | 429 | 501) {
        return status < 500;
    }
    match client
        .request(Method::GET, target)
        .timeout(Duration::from_millis(2500))
        .send()
        .await
    {
        Ok(response) => response.status().as_u16() < 500,
        Err(error) => error.status().is_some_and(|status| status.as_u16() < 500),
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_redirect;

    #[test]
    fn redirects_keep_relative_targets_local_and_encode_spaces() {
        assert_eq!(normalize_redirect("/dashboard"), "/dashboard");
        assert_eq!(
            normalize_redirect("/search?q=hello world"),
            "/search?q=hello%20world"
        );
        assert_eq!(normalize_redirect("//example.com"), "/");
        assert_eq!(normalize_redirect("javascript:alert(1)"), "/");
    }

    #[test]
    fn absolute_oidc_authorization_urls_are_preserved() {
        let target = "https://identity.example/authorize?state=abc";
        assert_eq!(normalize_redirect(target), target);
    }
}
