import yaml

from .status import get_local_ip

CONFIG_PATH = "config.yml"
AUTH_CONFIG_PATH = "auth.yml"


def _load_yaml_file(path: str) -> dict:
    try:
        with open(path, encoding="utf-8") as f:
            data = yaml.safe_load(f) or {}
            return data if isinstance(data, dict) else {}
    except FileNotFoundError:
        return {}


def _resolve_lan_host(cfg: dict) -> str | None:
    override = cfg.get("lan_host")
    if override:
        return str(override)
    return get_local_ip()


def _normalize_services(services: list, lan_host: str | None) -> list:
    normalized = []
    for svc in services:
        if not isinstance(svc, dict):
            continue
        svc = dict(svc)
        lan_port = svc.pop("lan_port", None)
        if lan_port and lan_host:
            svc["lan_url"] = f"http://{lan_host}:{lan_port}"
        normalized.append(svc)
    return normalized


def load_config() -> dict:
    cfg = _load_yaml_file(CONFIG_PATH)
    auth_file_cfg = _load_yaml_file(AUTH_CONFIG_PATH)

    auth_cfg = auth_file_cfg.get("auth", auth_file_cfg)
    if not isinstance(auth_cfg, dict):
        auth_cfg = {}

    cfg.pop("auth", None)
    cfg["auth"] = auth_cfg

    services = cfg.get("services", [])
    if isinstance(services, list):
        cfg["services"] = _normalize_services(services, _resolve_lan_host(cfg))

    return cfg
