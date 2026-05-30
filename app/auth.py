import os
import time
from functools import wraps
from threading import Lock
from urllib.parse import urlparse

from flask import Blueprint, current_app, redirect, render_template, request, session, url_for
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
        self.secure_cookie = (
            str(auth_cfg.get("secure_cookie", False)).lower() == "true"
        )
        self.username = auth_cfg.get("username")
        self.password_hash = auth_cfg.get("password_hash")
        self.login_max_attempts = max(1, int(bot_cfg.get("login_max_attempts", 5)))
        self.login_window_seconds = max(
            10, int(bot_cfg.get("login_window_seconds", 300))
        )
        self.login_lockout_seconds = max(
            30, int(bot_cfg.get("login_lockout_seconds", 900))
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

        if not self.username or not self.password_hash:
            raise RuntimeError(
                "Authentication requires auth.username and auth.password_hash in auth.yml."
            )

    def get_client_ip(self) -> str:
        forwarded = request.headers.get("X-Forwarded-For", "")
        if forwarded:
            return forwarded.split(",", 1)[0].strip() or "unknown"
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

    def lockout_remaining_seconds(self, client_ip: str) -> int:
        now = time.time()
        with self.attempts_lock:
            until = self.lockouts.get(client_ip, 0)
            if until <= now:
                self.lockouts.pop(client_ip, None)
                return 0
            return int(until - now)

    def register_failed_login(self, client_ip: str) -> None:
        now = time.time()
        threshold = now - self.login_window_seconds
        with self.attempts_lock:
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


def init_auth(app, cfg: dict) -> AuthManager:
    manager = AuthManager(cfg)
    app.extensions[AUTH_MANAGER_KEY] = manager
    app.config["SECRET_KEY"] = manager.secret_key
    app.config["SESSION_COOKIE_HTTPONLY"] = True
    app.config["SESSION_COOKIE_SAMESITE"] = "Lax"
    app.config["SESSION_COOKIE_SECURE"] = manager.secure_cookie
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


@auth_bp.route("/login", methods=["GET", "POST"])
def login():
    manager = get_auth_manager()
    cfg = current_app.config[APP_CONFIG_KEY]
    error = None
    if request.method == "GET" and manager.is_disallowed_user_agent():
        return ("Bot access denied", 403)

    if request.method == "POST":
        client_ip = manager.get_client_ip()
        lockout_seconds = manager.lockout_remaining_seconds(client_ip)
        if lockout_seconds > 0:
            wait_minutes = max(1, (lockout_seconds + 59) // 60)
            error = f"Too many failed attempts. Try again in {wait_minutes} minute(s)."
            return render_template("login.html", cfg=cfg, error=error), 429

        if request.form.get("website", "").strip():
            manager.register_failed_login(client_ip)
            error = "Invalid username or password"
            return render_template("login.html", cfg=cfg, error=error), 401

        submitted_username = request.form.get("username", "")
        submitted_password = request.form.get("password", "")

        if submitted_username == manager.username and check_password_hash(
            manager.password_hash, submitted_password
        ):
            manager.clear_failed_logins(client_ip)
            session["authenticated"] = True
            session.permanent = True
            next_url = request.args.get("next", "/")
            return redirect(
                next_url if is_safe_next_url(next_url) else url_for("pages.index")
            )

        manager.register_failed_login(client_ip)
        error = "Invalid username or password"

    status_code = 401 if error else 200
    return render_template("login.html", cfg=cfg, error=error), status_code


@auth_bp.post("/logout")
def logout():
    session.clear()
    return redirect(url_for("auth.login"))