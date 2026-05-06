## Raspberrypi Homepage
Simple site to list all self hosted apps on my raspberry pi server at one place with nice UI

## Authentication
This homepage now requires login before the services list is visible.

Set these values in `config.yml` under `auth`:
- `username`: login username
- `password_hash`: hashed password (Werkzeug format)
- `secret_key`: random long secret used to sign sessions
- `session_days`: how long login stays valid (default `30`)
- `secure_cookie`: set `true` when the site is served over HTTPS

Current default credentials in `config.yml` are only a starter and should be changed.

## Crawler and Bot Protection
Protected pages are already behind login, but the app now also adds layered bot defenses:
- `X-Robots-Tag: noindex, nofollow, noarchive, nosnippet, noimageindex`
- `<meta name="robots" ...>` on both login and protected pages
- `robots.txt` with `Disallow: /`
- login throttling and temporary IP lockout after repeated failed attempts
- hidden honeypot login field to trap basic form bots

Configure in `config.yml` with optional `bot_protection` values:
- `block_known_crawlers` (default `true`)
- `blocked_user_agents` (default `[]`)
- `login_max_attempts` (default `5`)
- `login_window_seconds` (default `300`)
- `login_lockout_seconds` (default `900`)

<img width="800" alt="{E1408C7C-993F-4342-9587-C11BECAE04F7}" src="https://github.com/user-attachments/assets/63744824-43e7-4330-86db-6c9cf7cc9b78" />

