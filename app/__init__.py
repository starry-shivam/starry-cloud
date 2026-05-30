from datetime import timedelta
import os

from flask import Flask, request

from .api import STATUS_TIMEOUT_SECONDS_KEY, STATUS_WORKERS_KEY, api_bp
from .auth import APP_CONFIG_KEY, auth_bp, init_auth
from .config import load_config
from .routes import DEVICE_MODEL_KEY, SYSTEM_HOSTNAME_KEY, pages_bp
from .status import (
    _read_device_model,
    _read_system_hostname,
)

_HERE = os.path.dirname(os.path.abspath(__file__))
_ROOT = os.path.dirname(_HERE)


def build_app() -> Flask:
    cfg = load_config()
    flask_app = Flask(
        __name__,
        static_folder=os.path.join(_ROOT, "static"),
        template_folder=os.path.join(_ROOT, "templates"),
    )
    system_hostname = _read_system_hostname()
    device_model = _read_device_model()
    status_timeout_seconds = float(cfg.get("status_timeout_seconds", 2.5))
    status_workers = int(cfg.get("status_workers", 8))

    auth_manager = init_auth(flask_app, cfg)

    flask_app.config[APP_CONFIG_KEY] = cfg
    flask_app.config[SYSTEM_HOSTNAME_KEY] = system_hostname
    flask_app.config[DEVICE_MODEL_KEY] = device_model
    flask_app.config[STATUS_TIMEOUT_SECONDS_KEY] = status_timeout_seconds
    flask_app.config[STATUS_WORKERS_KEY] = status_workers
    flask_app.config["PERMANENT_SESSION_LIFETIME"] = timedelta(
        days=max(auth_manager.session_days, 1)
    )

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

    flask_app.register_blueprint(pages_bp)
    flask_app.register_blueprint(api_bp)
    flask_app.register_blueprint(auth_bp)

    return flask_app


app = build_app()
