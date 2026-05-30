from concurrent.futures import ThreadPoolExecutor

from flask import Blueprint, current_app, jsonify

from .auth import APP_CONFIG_KEY, login_required
from .status import get_system_stats, is_service_online

STATUS_TIMEOUT_SECONDS_KEY = "APP_STATUS_TIMEOUT_SECONDS"
STATUS_WORKERS_KEY = "APP_STATUS_WORKERS"

api_bp = Blueprint("api", __name__, url_prefix="/api")


@api_bp.get("/service-status")
@login_required
def service_status():
    cfg = current_app.config[APP_CONFIG_KEY]
    services = cfg.get("services", [])
    status_timeout_seconds = current_app.config[STATUS_TIMEOUT_SECONDS_KEY]
    status_workers = current_app.config[STATUS_WORKERS_KEY]

    def check_one(idx_and_service):
        idx, service = idx_and_service
        service_url = service.get("url", "")
        online = is_service_online(service_url, timeout_seconds=status_timeout_seconds)
        return str(idx), online

    workers = max(1, min(status_workers, len(services) or 1))
    with ThreadPoolExecutor(max_workers=workers) as executor:
        results = dict(executor.map(check_one, enumerate(services)))
    response = jsonify({"statuses": results})
    response.headers["Cache-Control"] = "no-store, no-cache, must-revalidate, max-age=0"
    response.headers["Pragma"] = "no-cache"
    return response


@api_bp.get("/system-stats")
@login_required
def system_stats():
    response = jsonify(get_system_stats())
    response.headers["Cache-Control"] = "no-store, no-cache, must-revalidate, max-age=0"
    response.headers["Pragma"] = "no-cache"
    return response