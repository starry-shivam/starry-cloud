from flask import Blueprint, current_app, render_template

from .auth import APP_CONFIG_KEY, login_required

SYSTEM_HOSTNAME_KEY = "APP_SYSTEM_HOSTNAME"
DEVICE_MODEL_KEY = "APP_DEVICE_MODEL"

pages_bp = Blueprint("pages", __name__)


@pages_bp.route("/", methods=["GET", "HEAD"])
@login_required
def index():
    cfg = current_app.config[APP_CONFIG_KEY]
    return render_template(
        "index.html",
        cfg=cfg,
        services=cfg.get("services", []),
        system_hostname=current_app.config[SYSTEM_HOSTNAME_KEY],
        device_model=current_app.config[DEVICE_MODEL_KEY],
    )


@pages_bp.get("/robots.txt")
def robots_txt():
    body = "User-agent: *\nDisallow: /\n"
    return current_app.response_class(body, mimetype="text/plain")