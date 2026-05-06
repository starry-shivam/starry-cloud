import os
from concurrent.futures import ThreadPoolExecutor
from datetime import timedelta
from functools import wraps
from urllib.error import HTTPError, URLError
from urllib.parse import urlparse
from urllib.request import Request, urlopen

import yaml
from flask import Flask, jsonify, redirect, render_template, request, session, url_for
from werkzeug.security import check_password_hash

CONFIG_PATH = "config.yml"


def load_config() -> dict:
    with open(CONFIG_PATH, encoding="utf-8") as f:
        return yaml.safe_load(f) or {}


def is_safe_next_url(target: str) -> bool:
    if not target:
        return False
    parsed = urlparse(target)
    return parsed.scheme == "" and parsed.netloc == "" and target.startswith("/")


def is_service_online(url: str, timeout_seconds: float = 2.5) -> bool:
    req = Request(url, method="HEAD")
    try:
        with urlopen(req, timeout=timeout_seconds) as response:
            return response.status < 500
    except HTTPError as err:
        return err.code < 500
    except (URLError, TimeoutError, ValueError):
        return False


def build_app() -> Flask:
    cfg = load_config()
    app = Flask(__name__, static_folder="static", template_folder="templates")

    auth_cfg = cfg.get("auth", {})
    secret_key = auth_cfg.get("secret_key") or os.environ.get("SECRET_KEY")
    if not secret_key:
        raise RuntimeError(
            "Authentication requires auth.secret_key in config.yml or SECRET_KEY environment variable."
        )

    session_days = int(auth_cfg.get("session_days", 30))
    status_timeout_seconds = float(cfg.get("status_timeout_seconds", 2.5))
    status_workers = int(cfg.get("status_workers", 8))

    app.config["SECRET_KEY"] = secret_key
    app.config["SESSION_COOKIE_HTTPONLY"] = True
    app.config["SESSION_COOKIE_SAMESITE"] = "Lax"
    app.config["SESSION_COOKIE_SECURE"] = (
        str(auth_cfg.get("secure_cookie", False)).lower() == "true"
    )
    app.config["PERMANENT_SESSION_LIFETIME"] = timedelta(days=max(session_days, 1))

    username = auth_cfg.get("username")
    password_hash = auth_cfg.get("password_hash")

    if not username or not password_hash:
        raise RuntimeError(
            "Authentication requires auth.username and auth.password_hash in config.yml."
        )

    def login_required(view_func):
        @wraps(view_func)
        def wrapped(*args, **kwargs):
            if session.get("authenticated"):
                return view_func(*args, **kwargs)
            return redirect(url_for("login", next=request.path))

        return wrapped

    @app.route("/", methods=["GET", "HEAD"])
    @login_required
    def index():
        return render_template("index.html", cfg=cfg, services=cfg.get("services", []))

    @app.get("/api/service-status")
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

    @app.route("/login", methods=["GET", "POST"])
    def login():
        error = None

        if request.method == "POST":
            submitted_username = request.form.get("username", "")
            submitted_password = request.form.get("password", "")

            if submitted_username == username and check_password_hash(
                password_hash, submitted_password
            ):
                session["authenticated"] = True
                session.permanent = True
                next_url = request.args.get("next", "/")
                return redirect(
                    next_url if is_safe_next_url(next_url) else url_for("index")
                )

            error = "Invalid username or password"

        return render_template("login.html", cfg=cfg, error=error)

    @app.post("/logout")
    def logout():
        session.clear()
        return redirect(url_for("login"))

    return app


app = build_app()


if __name__ == "__main__":
    app.run(host="0.0.0.0", port=5000)
