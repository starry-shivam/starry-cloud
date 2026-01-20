import yaml
import os
import shutil
from jinja2 import Environment, FileSystemLoader

CONFIG_PATH = "config.yml"
TEMPLATE_DIR = "templates"
OUTPUT_DIR = "site"

# Load YAML
with open(CONFIG_PATH, encoding="utf-8") as f:
    cfg = yaml.safe_load(f)

# Render HTML
env = Environment(loader=FileSystemLoader(TEMPLATE_DIR))
template = env.get_template("index.html")

html = template.render(
    cfg=cfg,
    services=cfg.get("services", [])
)

# Create output folder
os.makedirs(OUTPUT_DIR, exist_ok=True)

# Write rendered file
with open(f"{OUTPUT_DIR}/index.html", "w", encoding="utf-8") as f:
    f.write(html)

# Copy static files
if os.path.exists("static"):
    shutil.copytree("static", f"{OUTPUT_DIR}/static", dirs_exist_ok=True)

print("Site generated successfully.")