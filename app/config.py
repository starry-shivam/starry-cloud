import yaml

CONFIG_PATH = "config.yml"
AUTH_CONFIG_PATH = "auth.yml"


def _load_yaml_file(path: str) -> dict:
    try:
        with open(path, encoding="utf-8") as f:
            data = yaml.safe_load(f) or {}
            return data if isinstance(data, dict) else {}
    except FileNotFoundError:
        return {}


def load_config() -> dict:
    cfg = _load_yaml_file(CONFIG_PATH)
    auth_file_cfg = _load_yaml_file(AUTH_CONFIG_PATH)

    auth_cfg = auth_file_cfg.get("auth", auth_file_cfg)
    if not isinstance(auth_cfg, dict):
        auth_cfg = {}

    cfg.pop("auth", None)
    cfg["auth"] = auth_cfg
    return cfg
