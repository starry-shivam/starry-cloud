# Starry Cloud

<img width="1264" height="674" alt="IMG_20260528_100545_970" src="https://github.com/user-attachments/assets/df0b8122-95d5-4ece-9f80-d883f463b544" />

A self-hosted dashboard for personal services with live status checks, system resource monitoring, and password-protected access. Useful organizing and monitoring your self-hosted apps in one cozy place.

### Quick Start

Start the app with Docker Compose:

```bash
cp config.example.yml config.yml
cp auth.example.yml auth.yml
docker compose run --rm starry-cloud python3 gen_auth.py
docker compose up -d --build
```

The app will be available at `http://localhost:5000`.

### Configuration

The app uses two config files:

- `config.yml` for dashboard content and app behavior.
- `auth.yml` for authentication settings.

Use the examples as a starting point:

```bash
cp config.example.yml config.yml
cp auth.example.yml auth.yml
```

Notes:

- `status_timeout_seconds` controls how long each service probe can take before it is treated as offline.
- `status_workers` controls how many service checks run in parallel.
- `auth.secret_key` can be omitted if `SECRET_KEY` is provided in the environment.
- `auth.password_hash` should be a Werkzeug-compatible password hash rather than a plain-text password.
