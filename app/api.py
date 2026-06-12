import json
from concurrent.futures import ThreadPoolExecutor, as_completed

from flask import Blueprint, Response, current_app, jsonify, request

from .auth import APP_CONFIG_KEY, login_required
from .status import get_system_stats, is_service_online

_PROBE_TIMEOUT_SECONDS = 2.5

api_bp = Blueprint("api", __name__, url_prefix="/api")


def _set_no_cache_headers(response):
    response.headers["Cache-Control"] = "no-store, no-cache, must-revalidate, max-age=0"
    response.headers["Pragma"] = "no-cache"
    return response


@api_bp.get("/service-status")
@login_required
def service_status():
    cfg = current_app.config[APP_CONFIG_KEY]
    services = cfg.get("services", [])
    stream_response = request.args.get("stream") == "1"

    def check_one(idx_and_service):
        idx, service = idx_and_service
        service_url = service.get("url", "")
        online = is_service_online(service_url, timeout_seconds=_PROBE_TIMEOUT_SECONDS)
        return str(idx), online

    workers = max(1, len(services))

    def generate_status_lines():
        with ThreadPoolExecutor(max_workers=workers) as executor:
            futures = {
                executor.submit(check_one, item): item[0]
                for item in enumerate(services)
            }
            for future in as_completed(futures):
                service_id = str(futures[future])
                try:
                    _, online = future.result()
                except Exception:
                    online = None
                yield json.dumps({"id": service_id, "online": online}) + "\n"

    if stream_response:
        response = Response(
            generate_status_lines(),
            mimetype="application/x-ndjson",
        )
        response.headers["X-Accel-Buffering"] = "no"
        return _set_no_cache_headers(response)

    with ThreadPoolExecutor(max_workers=workers) as executor:
        results = dict(executor.map(check_one, enumerate(services)))
    response = jsonify({"statuses": results})
    return _set_no_cache_headers(response)


@api_bp.get("/system-stats")
@login_required
def system_stats():
    response = jsonify(get_system_stats())
    return _set_no_cache_headers(response)
