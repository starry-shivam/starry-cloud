
from flask import Flask, render_template
import yaml
import os

app = Flask(__name__)

CONFIG_PATH = os.environ.get("STARRYCLOUD_CONFIG", "config.yml")
_config = None


def load_config(path=CONFIG_PATH):
    global _config
    if _config is None:
        with open(path, "r") as f:
            _config = yaml.safe_load(f)
    return _config


@app.route("/")
def index():
    cfg = load_config()
    services = cfg.get("services", [])
    return render_template("index.html", cfg=cfg, services=services)


if __name__ == "__main__":
    app.run(host="0.0.0.0", port=5000, debug=True)
