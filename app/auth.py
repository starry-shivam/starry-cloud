import os
import secrets
import time
from functools import wraps
from threading import Lock
from urllib.parse import urlparse

from authlib.integrations.base_client.errors import OAuthError
from authlib.integrations.flask_client import OAuth
from flask import (
    Blueprint,
    current_app,
    flash,
    get_flashed_messages,
    redirect,
    render_template,
    request,
    session,
    url_for,
)
from werkzeug.security import check_password_hash

KNOWN_CRAWLER_SIGNATURES = (
    "googlebot",
    "bingbot",
    "yandex",
    "baiduspider",
    "duckduckbot",
    "facebookexternalhit",
    "ia_archiver",
    "slurp",
    "crawler",
    "spider",
    "bot",
    "curl",
    "wget",
)

AUTH_MANAGER_KEY = "starry_cloud_auth_manager"
APP_CONFIG_KEY = "APP_CONFIG"

auth_bp = Blueprint("auth", __name__)


def is_safe_next_url(target: str) -> bool:
    if not target:
        return False
    parsed = urlparse(target)
    return parsed.scheme == "" and parsed.netloc == "" and target.startswith("/")


class AuthManager:
    def __init__(self, cfg: dict):
        auth_cfg = cfg.get("auth", {})
        bot_cfg = cfg.get("bot_protection", {})

        secret_key = auth_cfg.get("secret_key") or os.environ.get("SECRET_KEY")
        if not secret_key:
            raise RuntimeError(
                "Authentication requires auth.secret_key in auth.yml or SECRET_KEY environment variable."
            )

        self.secret_key = secret_key
        self.session_days = int(auth_cfg.get("session_days", 30))
        self.secure_cookie = str(auth_cfg.get("secure_cookie", False)).lower() == "true"
        self.password_enabled = (
            str(auth_cfg.get("password_enabled", True)).lower() == "true"
        )
        self.username = auth_cfg.get("username")
        self.password_hash = auth_cfg.get("password_hash")
        oidc_cfg = auth_cfg.get("oidc") or {}
        if not isinstance(oidc_cfg, dict):
            raise RuntimeError("auth.oidc must be a mapping.")
        self.oidc_enabled = str(oidc_cfg.get("enabled", False)).lower() == "true"
        self.oidc_provider_name = str(oidc_cfg.get("provider_name") or "OIDC").strip()
        if not self.oidc_provider_name:
            self.oidc_provider_name = "OIDC"
        self.oidc_discovery_url = oidc_cfg.get("discovery_url")
        self.oidc_client_id = oidc_cfg.get("client_id")
        self.oidc_client_secret = oidc_cfg.get("client_secret")
        oidc_scopes = str(oidc_cfg.get("scope", "openid email profile")).split()
        if "openid" not in oidc_scopes:
            oidc_scopes.insert(0, "openid")
        self.oidc_scope = " ".join(oidc_scopes)
        allowed_emails = oidc_cfg.get("allowed_emails") or []
        if not isinstance(allowed_emails, list):
            raise RuntimeError("auth.oidc.allowed_emails must be a list.")
        self.oidc_allowed_emails = {
            str(email).strip().lower()
            for email in allowed_emails
            if str(email).strip()
        }
        self.login_max_attempts = max(1, int(bot_cfg.get("login_max_attempts", 5)))
        self.login_window_seconds = max(
            10, int(bot_cfg.get("login_window_seconds", 300))
        )
        self.login_lockout_seconds = max(
            30, int(bot_cfg.get("login_lockout_seconds", 900))
        )
        self.state_cleanup_interval_seconds = max(
            30, int(bot_cfg.get("state_cleanup_interval_seconds", 300))
        )
        self.block_known_crawlers = bool(bot_cfg.get("block_known_crawlers", True))
        self.blocked_user_agents = [
            str(item).lower().strip()
            for item in bot_cfg.get("blocked_user_agents", [])
            if str(item).strip()
        ]
        self.failed_login_attempts = {}
        self.lockouts = {}
        self.attempts_lock = Lock()
        self._last_state_cleanup = 0.0

        if self.password_enabled and (not self.username or not self.password_hash):
            raise RuntimeError(
                "Authentication requires auth.username and auth.password_hash in auth.yml."
            )
        if self.oidc_enabled and not all(
            (self.oidc_discovery_url, self.oidc_client_id, self.oidc_client_secret)
        ):
            raise RuntimeError(
                "OIDC requires auth.oidc.discovery_url, client_id, and client_secret."
            )
        if not self.password_enabled and not self.oidc_enabled:
            raise RuntimeError("At least one authentication method must be enabled.")

    def get_client_ip(self) -> str:
        # Use remote_addr only. If the app is behind trusted proxies,
        # ProxyFix rewrites remote_addr safely based on configured hop count.
        return request.remote_addr or "unknown"

    def is_disallowed_user_agent(self) -> bool:
        if not self.block_known_crawlers:
            return False

        user_agent = request.headers.get("User-Agent", "").lower()
        if not user_agent:
            return True

        if any(token in user_agent for token in self.blocked_user_agents):
            return True

        return any(token in user_agent for token in KNOWN_CRAWLER_SIGNATURES)

    def _cleanup_state_if_due(self, now: float) -> None:
        if now - self._last_state_cleanup < self.state_cleanup_interval_seconds:
            return

        attempt_threshold = now - self.login_window_seconds
        self.failed_login_attempts = {
            ip: [ts for ts in attempts if ts >= attempt_threshold]
            for ip, attempts in self.failed_login_attempts.items()
            if any(ts >= attempt_threshold for ts in attempts)
        }
        self.lockouts = {
            ip: until for ip, until in self.lockouts.items() if until > now
        }
        self._last_state_cleanup = now

    def lockout_remaining_seconds(self, client_ip: str) -> int:
        now = time.time()
        with self.attempts_lock:
            self._cleanup_state_if_due(now)
            until = self.lockouts.get(client_ip, 0)
            if until <= now:
                self.lockouts.pop(client_ip, None)
                return 0
            return max(1, int(until - now))

    def register_failed_login(self, client_ip: str) -> None:
        now = time.time()
        threshold = now - self.login_window_seconds
        with self.attempts_lock:
            self._cleanup_state_if_due(now)
            attempts = [
                ts
                for ts in self.failed_login_attempts.get(client_ip, [])
                if ts >= threshold
            ]
            attempts.append(now)
            self.failed_login_attempts[client_ip] = attempts
            if len(attempts) >= self.login_max_attempts:
                self.lockouts[client_ip] = now + self.login_lockout_seconds
                self.failed_login_attempts.pop(client_ip, None)

    def clear_failed_logins(self, client_ip: str) -> None:
        with self.attempts_lock:
            self.failed_login_attempts.pop(client_ip, None)
            self.lockouts.pop(client_ip, None)


def generate_csrf_token() -> str:
    if "_csrf_token" not in session:
        session["_csrf_token"] = secrets.token_hex(32)
    return session["_csrf_token"]


def validate_csrf_token() -> bool:
    token = session.get("_csrf_token", "")
    submitted = request.form.get("csrf_token", "")
    if not token or not submitted:
        return False
    return secrets.compare_digest(token, submitted)


def init_auth(app, cfg: dict) -> AuthManager:
    manager = AuthManager(cfg)
    app.extensions[AUTH_MANAGER_KEY] = manager
    app.config["SECRET_KEY"] = manager.secret_key
    app.config["SESSION_COOKIE_HTTPONLY"] = True
    app.config["SESSION_COOKIE_SAMESITE"] = "Lax"
    app.config["SESSION_COOKIE_SECURE"] = manager.secure_cookie

    if manager.oidc_enabled:
        oauth = OAuth(app)
        manager.oidc_client = oauth.register(
            name="oidc",
            client_id=manager.oidc_client_id,
            client_secret=manager.oidc_client_secret,
            server_metadata_url=manager.oidc_discovery_url,
            client_kwargs={
                "scope": manager.oidc_scope,
                "code_challenge_method": "S256",
            },
        )

    @app.context_processor
    def inject_csrf_token():
        return {"csrf_token": generate_csrf_token}

    return manager


def get_auth_manager() -> AuthManager:
    return current_app.extensions[AUTH_MANAGER_KEY]


def login_required(view_func):
    @wraps(view_func)
    def wrapped(*args, **kwargs):
        manager = get_auth_manager()
        if manager.is_disallowed_user_agent():
            return ("Bot access denied", 403)
        if session.get("authenticated"):
            return view_func(*args, **kwargs)
        return redirect(url_for("auth.login", next=request.path))

    return wrapped


def _flash_error_or_lockout(manager: "AuthManager", client_ip: str) -> None:
    lockout_seconds = manager.lockout_remaining_seconds(client_ip)
    if lockout_seconds > 0:
        wait_minutes = max(1, (lockout_seconds + 59) // 60)
        flash(f"Too many failed attempts. Try again in {wait_minutes} minute(s).")
    else:
        flash("Invalid username or password")


@auth_bp.route("/login", methods=["GET", "POST"])
def login():
    manager = get_auth_manager()
    cfg = current_app.config[APP_CONFIG_KEY]

    if request.method == "GET":
        if manager.is_disallowed_user_agent():
            return ("Bot access denied", 403)
        messages = get_flashed_messages()
        error = messages[0] if messages else None
        return render_template(
            "login.html",
            cfg=cfg,
            error=error,
            password_enabled=manager.password_enabled,
            oidc_enabled=manager.oidc_enabled,
            oidc_provider_name=manager.oidc_provider_name,
        )

    if not manager.password_enabled:
        return ("Password authentication is disabled", 404)

    # POST always redirect after processing (PRG pattern) so a browser
    # refresh or server-restart replay cannot resubmit credentials.
    client_ip = manager.get_client_ip()
    next_url = request.args.get("next", "")
    login_url = (
        url_for("auth.login", next=next_url) if next_url else url_for("auth.login")
    )

    if not validate_csrf_token():
        flash("Invalid or missing security token.")
        return redirect(login_url)

    lockout_seconds = manager.lockout_remaining_seconds(client_ip)
    if lockout_seconds > 0:
        wait_minutes = max(1, (lockout_seconds + 59) // 60)
        flash(f"Too many failed attempts. Try again in {wait_minutes} minute(s).")
        return redirect(login_url)

    if request.form.get("website", "").strip():
        manager.register_failed_login(client_ip)
        _flash_error_or_lockout(manager, client_ip)
        return redirect(login_url)

    submitted_username = request.form.get("username", "")
    submitted_password = request.form.get("password", "")

    username_ok = secrets.compare_digest(submitted_username, manager.username)
    password_ok = check_password_hash(manager.password_hash, submitted_password)

    if username_ok and password_ok:
        manager.clear_failed_logins(client_ip)
        session.clear()
        session["authenticated"] = True
        session["_csrf_token"] = secrets.token_hex(32)
        session.permanent = True
        return redirect(
            next_url if is_safe_next_url(next_url) else url_for("pages.index")
        )

    manager.register_failed_login(client_ip)
    _flash_error_or_lockout(manager, client_ip)
    return redirect(login_url)


@auth_bp.get("/login/oidc")
def oidc_login():
    manager = get_auth_manager()
    if not manager.oidc_enabled:
        return ("OIDC authentication is disabled", 404)
    if manager.is_disallowed_user_agent():
        return ("Bot access denied", 403)

    next_url = request.args.get("next", "")
    session["oidc_next"] = next_url if is_safe_next_url(next_url) else ""
    callback_url = url_for("auth.oidc_callback", _external=True)
    return manager.oidc_client.authorize_redirect(callback_url)


@auth_bp.get("/login/oidc/callback")
def oidc_callback():
    manager = get_auth_manager()
    if not manager.oidc_enabled:
        return ("OIDC authentication is disabled", 404)

    try:
        token = manager.oidc_client.authorize_access_token()
    except OAuthError as exc:
        current_app.logger.warning(
            "OIDC token exchange or state validation failed (%s)",
            type(exc).__name__,
        )
        session.pop("oidc_next", None)
        flash("OIDC sign-in failed. Please try again.")
        return redirect(url_for("auth.login"))

    userinfo = token.get("userinfo")
    if not isinstance(userinfo, dict) or not userinfo.get("sub"):
        claim_names = sorted(userinfo.keys()) if isinstance(userinfo, dict) else []
        current_app.logger.warning(
            "OIDC response did not contain a valid user identity "
            "(userinfo_type=%s, claim_names=%s)",
            type(userinfo).__name__,
            claim_names,
        )
        session.pop("oidc_next", None)
        flash("The identity provider did not return a valid user identity.")
        return redirect(url_for("auth.login"))

    if manager.oidc_allowed_emails:
        email = str(userinfo.get("email", "")).strip().lower()
        if (
            email not in manager.oidc_allowed_emails
            or userinfo.get("email_verified") is not True
        ):
            verified_claim = userinfo.get("email_verified")
            verified_state = (
                "true"
                if verified_claim is True
                else "false"
                if verified_claim is False
                else "missing"
                if verified_claim is None
                else type(verified_claim).__name__
            )
            current_app.logger.warning(
                "OIDC access denied by email allowlist "
                "(email_matched=%s, email_verified=%s)",
                email in manager.oidc_allowed_emails,
                verified_state,
            )
            session.pop("oidc_next", None)
            flash("This account is not allowed to sign in.")
            return redirect(url_for("auth.login"))

    next_url = session.pop("oidc_next", "")
    session.clear()
    session["authenticated"] = True
    session["_csrf_token"] = secrets.token_hex(32)
    session.permanent = True
    return redirect(next_url if is_safe_next_url(next_url) else url_for("pages.index"))


@auth_bp.post("/logout")
def logout():
    if not validate_csrf_token():
        return ("", 400)
    session.clear()
    return redirect(url_for("auth.login"))
