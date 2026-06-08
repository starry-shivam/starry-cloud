# Starry Cloud

A self-hosted dashboard for my personal services with live status checks, system resource monitoring, and password-protected access. Useful organizing and monitoring self-hosted apps in one cozy place.

<img width="1264" height="670" alt="IMG_20260608_162548" src="https://github.com/user-attachments/assets/34a8760e-7818-4bb1-99a8-d315fc5976dc" />

---

### Configuration

The app uses two config files:

- `config.yml` for dashboard content and app behavior.
- `auth.yml` for authentication settings.

### Quick Start

Deploy the app with Docker Compose in under a minute:

```bash
# Create dashboard config file:
cp config.example.yml config.yml

# Create auth file:
touch auth.yml

# build the docker image
docker compose build

# Generate auth file contents and paste in auth.yml
docker compose run --rm starry-cloud python3 gen_auth.py

# Start the app
docker compose up -d
```

The app will be available at `http://localhost:5000`.


