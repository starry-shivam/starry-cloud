# Starry Cloud

A self-hosted dashboard for my personal services with live status checks, system resource monitoring, and password-protected access. Useful organizing and monitoring self-hosted apps in one cozy place.

<img width="1200" alt="Screenshot From 2026-05-30 11-27-43" src="https://github.com/user-attachments/assets/6ff1872a-44c2-4993-9d34-f42689d1a4fc" />

---

### Quick Start

Deploy the app with Docker Compose in under a minute:

```bash
# Create dashboard config file:
cp config.example.yml config.yml

# Create auth file:
touch auth.yml

# Generate auth file contents and paste in auth.yml
docker compose run --rm starry-cloud python3 gen_auth.py

# Start the app (omit --build in subsequent runs)
docker compose up -d --build
```

The app will be available at `http://localhost:5000`.

### Configuration

The app uses two config files:

- `config.yml` for dashboard content and app behavior.
- `auth.yml` for authentication settings.

Notes:

- `status_timeout_seconds` controls how long each service probe can take before it is treated as offline.
- `status_workers` controls how many service checks run in parallel.
- `auth.secret_key` can be omitted if `SECRET_KEY` is provided in the environment.
- `auth.password_hash` should be a Werkzeug-compatible password hash rather than a plain-text password.
