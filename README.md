# Starry Cloud

A self-hosted dashboard for personal services with live status checks, system resource monitoring, and protected access.

<img width="1264" height="681" alt="Screenshot 2026-09-30 164536" src="https://github.com/user-attachments/assets/eca6afae-89dc-4619-aa6a-0b54e93b0e63" />


## Configuration

The app uses `config.yml` for dashboard content and `auth.yml` for authentication settings. Create these from the examples before starting:

```sh
cp config.example.yml config.yml
cp auth.example.yml auth.yml
```

Authentication supports username/password and OpenID Connect (OIDC). Configure OIDC under `auth.oidc` in `auth.yml` with the provider's discovery URL, client ID, and client secret. Register `https://your-dashboard.example.com/login/oidc/callback` as an allowed redirect URI at the identity provider, replacing the host with the public dashboard URL.

Set `auth.password_enabled: false` to disable password login; at least one authentication method must remain enabled. By default, any account the identity provider permits can sign in. Set `auth.oidc.allowed_emails` to restrict access to verified email addresses.

## Run

Build the docker image
```
docker compose build
```

Generate password and signing-key settings for `auth.yml` with the bundled Rust binary:

```sh
docker compose run --rm starry-cloud gen-auth
```

Start the service:

```sh
docker compose up -d
```

The dashboard is available at `http://localhost:5000`.


