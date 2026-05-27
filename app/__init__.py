import os
import time
from concurrent.futures import ThreadPoolExecutor
from datetime import timedelta
from functools import wraps
from threading import Lock
from urllib.parse import urlparse

import yaml
from flask import Flask, jsonify, redirect, render_template, request, session, url_for
from werkzeug.security import check_password_hash

from .status import (
    _read_device_model,
    _read_system_hostname,
    get_system_stats,
    is_service_online,
)

CONFIG_PATH = "config.yml"

_HERE = os.path.dirname(os.path.abspath(__file__))
_ROOT = os.path.dirname(_HERE)

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


def load_config() -> dict:
    with open(CONFIG_PATH, encoding="utf-8") as f:
        return yaml.safe_load(f) or {}


def is_safe_next_url(target: str) -> bool:
    if not target:
        return False
    parsed = urlparse(target)
    return parsed.scheme == "" and parsed.netloc == "" and target.startswith("/")


def build_app() -> Flask:
    cfg = load_config()
    flask_app = Flask(
        __name__,
        static_folder=os.path.join(_ROOT, "static"),
        template_folder=os.path.join(_ROOT, "templates"),
    )
    system_hostname = _read_system_hostname()
    device_model = _read_device_model()

    auth_cfg = cfg.get("auth", {})
    secret_key = auth_cfg.get("secret_key") or os.environ.get("SECRET_KEY")
    if not secret_key:
        raise RuntimeError(
            "Authentication requires auth.secret_key in config.yml or SECRET_KEY environment variable."
        )

    session_days = int(auth_cfg.get("session_days", 30))
    status_timeout_seconds = float(cfg.get("status_timeout_seconds", 2.5))
    status_workers = int(cfg.get("status_workers", 8))

    flask_app.config["SECRET_KEY"] = secret_key
    flask_app.config["SESSION_COOKIE_HTTPONLY"] = True
    flask_app.config["SESSION_COOKIE_SAMESITE"] = "Lax"
    flask_app.config["SESSION_COOKIE_SECURE"] = (
        str(auth_cfg.get("secure_cookie", False)).lower() == "true"
    )
    flask_app.config["PERMANENT_SESSION_LIFETIME"] = timedelta(
        days=max(session_days, 1)
    )

    username = auth_cfg.get("username")
    password_hash = auth_cfg.get("password_hash")
    bot_cfg = cfg.get("bot_protection", {})
    login_max_attempts = max(1, int(bot_cfg.get("login_max_attempts", 5)))
    login_window_seconds = max(10, int(bot_cfg.get("login_window_seconds", 300)))
    login_lockout_seconds = max(30, int(bot_cfg.get("login_lockout_seconds", 900)))
    block_known_crawlers = bool(bot_cfg.get("block_known_crawlers", True))
    blocked_user_agents = [
        str(item).lower().strip()
        for item in bot_cfg.get("blocked_user_agents", [])
        if str(item).strip()
    ]

    failed_login_attempts = {}
    lockouts = {}
    attempts_lock = Lock()

    if not username or not password_hash:
        raise RuntimeError(
            "Authentication requires auth.username and auth.password_hash in config.yml."
        )

    def get_client_ip() -> str:
        forwarded = request.headers.get("X-Forwarded-For", "")
        if forwarded:
            return forwarded.split(",", 1)[0].strip() or "unknown"
        return request.remote_addr or "unknown"

    def is_disallowed_user_agent() -> bool:
        if not block_known_crawlers:
            return False

        user_agent = request.headers.get("User-Agent", "").lower()
        if not user_agent:
            return True

        if any(token in user_agent for token in blocked_user_agents):
            return True

        return any(token in user_agent for token in KNOWN_CRAWLER_SIGNATURES)

    def lockout_remaining_seconds(client_ip: str) -> int:
        now = time.time()
        with attempts_lock:
            until = lockouts.get(client_ip, 0)
            if until <= now:
                lockouts.pop(client_ip, None)
                return 0
            return int(until - now)

    def register_failed_login(client_ip: str) -> None:
        now = time.time()
        threshold = now - login_window_seconds
        with attempts_lock:
            attempts = [
                ts for ts in failed_login_attempts.get(client_ip, []) if ts >= threshold
            ]
            attempts.append(now)
            failed_login_attempts[client_ip] = attempts
            if len(attempts) >= login_max_attempts:
                lockouts[client_ip] = now + login_lockout_seconds
                failed_login_attempts.pop(client_ip, None)

    def clear_failed_logins(client_ip: str) -> None:
        with attempts_lock:
            failed_login_attempts.pop(client_ip, None)
            lockouts.pop(client_ip, None)

    @flask_app.after_request
    def add_security_headers(response):
        response.headers.setdefault(
            "X-Robots-Tag",
            "noindex, nofollow, noarchive, nosnippet, noimageindex",
        )
        if not request.path.startswith("/static/"):
            response.headers.setdefault(
                "Cache-Control", "no-store, no-cache, must-revalidate, max-age=0"
            )
            response.headers.setdefault("Pragma", "no-cache")
        return response

    def login_required(view_func):
        @wraps(view_func)
        def wrapped(*args, **kwargs):
            if is_disallowed_user_agent():
                return ("Bot access denied", 403)
            if session.get("authenticated"):
                return view_func(*args, **kwargs)
            return redirect(url_for("login", next=request.path))

        return wrapped

    @flask_app.route("/", methods=["GET", "HEAD"])
    @login_required
    def index():
        return render_template(
            "index.html",
            cfg=cfg,
            services=cfg.get("services", []),
            system_hostname=system_hostname,
            device_model=device_model,
        )

    @flask_app.get("/api/service-status")
    @login_required
    def service_status():
        services = cfg.get("services", [])

        def check_one(idx_and_service):
            idx, service = idx_and_service
            service_url = service.get("url", "")
            online = is_service_online(
                service_url, timeout_seconds=status_timeout_seconds
            )
            return str(idx), online

        workers = max(1, min(status_workers, len(services) or 1))
        with ThreadPoolExecutor(max_workers=workers) as executor:
            results = dict(executor.map(check_one, enumerate(services)))
        response = jsonify({"statuses": results})
        response.headers["Cache-Control"] = (
            "no-store, no-cache, must-revalidate, max-age=0"
        )
        response.headers["Pragma"] = "no-cache"
        return response

    @flask_app.get("/api/system-stats")
    @login_required
    def system_stats():
        response = jsonify(get_system_stats())
        response.headers["Cache-Control"] = (
            "no-store, no-cache, must-revalidate, max-age=0"
        )
        response.headers["Pragma"] = "no-cache"
        return response

    @flask_app.get("/robots.txt")
    def robots_txt():
        body = "User-agent: *\nDisallow: /\n"
        return flask_app.response_class(body, mimetype="text/plain")

    @flask_app.route("/login", methods=["GET", "POST"])
    def login():
        error = None
        if request.method == "GET" and is_disallowed_user_agent():
            return ("Bot access denied", 403)

        if request.method == "POST":
            client_ip = get_client_ip()
            lockout_seconds = lockout_remaining_seconds(client_ip)
            if lockout_seconds > 0:
                wait_minutes = max(1, (lockout_seconds + 59) // 60)
                error = (
                    f"Too many failed attempts. Try again in {wait_minutes} minute(s)."
                )
                return render_template("login.html", cfg=cfg, error=error), 429

            if request.form.get("website", "").strip():
                register_failed_login(client_ip)
                error = "Invalid username or password"
                return render_template("login.html", cfg=cfg, error=error), 401

            submitted_username = request.form.get("username", "")
            submitted_password = request.form.get("password", "")

            if submitted_username == username and check_password_hash(
                password_hash, submitted_password
            ):
                clear_failed_logins(client_ip)
                session["authenticated"] = True
                session.permanent = True
                next_url = request.args.get("next", "/")
                return redirect(
                    next_url if is_safe_next_url(next_url) else url_for("index")
                )

            register_failed_login(client_ip)
            error = "Invalid username or password"

        status_code = 401 if error else 200
        return render_template("login.html", cfg=cfg, error=error), status_code

    @flask_app.post("/logout")
    def logout():
        session.clear()
        return redirect(url_for("login"))

    return flask_app


app = build_app()
