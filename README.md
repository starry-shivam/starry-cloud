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

<img width="800" alt="{E1408C7C-993F-4342-9587-C11BECAE04F7}" src="https://github.com/user-attachments/assets/63744824-43e7-4330-86db-6c9cf7cc9b78" />

