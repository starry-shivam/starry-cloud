"""Generate auth settings for auth.yml.

Usage:
  python3 gen_auth.py
"""

from __future__ import annotations

import argparse
import getpass
import secrets
import sys

from werkzeug.security import generate_password_hash


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Interactive auth.yml generator.",
    )
    parser.add_argument(
        "--password",
        help="Plain-text password to hash. If omitted, you will be prompted securely.",
    )
    parser.add_argument(
        "--secret-key",
        help="Use a specific secret_key value instead of generating one.",
    )
    return parser.parse_args()


def read_password(password_arg: str | None) -> str:
    if password_arg:
        return password_arg

    password = getpass.getpass("Enter new password: ")
    confirm = getpass.getpass("Confirm password: ")
    if password != confirm:
        raise ValueError("Passwords do not match.")
    if not password:
        raise ValueError("Password cannot be empty.")
    return password


def prompt_string(label: str, default: str | None = None) -> str:
    suffix = f" [{default}]" if default else ""
    value = input(f"{label}{suffix}: ").strip()
    if value:
        return value
    return default or ""


def prompt_int(label: str, default: int) -> int:
    raw = prompt_string(label, str(default))
    try:
        value = int(raw)
    except ValueError as exc:
        raise ValueError(f"{label} must be an integer.") from exc
    if value < 1:
        raise ValueError(f"{label} must be at least 1.")
    return value


def prompt_bool(label: str, default: bool) -> bool:
    default_token = "Y/n" if default else "y/N"
    raw = input(f"{label} ({default_token}): ").strip().lower()
    if not raw:
        return default
    if raw in {"y", "yes", "true", "1"}:
        return True
    if raw in {"n", "no", "false", "0"}:
        return False
    raise ValueError(f"{label} must be yes or no.")


def yaml_quote(value: str) -> str:
    escaped = value.replace("\\", "\\\\").replace('"', '\\"')
    return f'"{escaped}"'


def main() -> int:
    args = parse_args()

    try:
        username = prompt_string("Username", "starry")
        if not username:
            raise ValueError("Username cannot be empty.")

        password = read_password(args.password)
        session_days = prompt_int("Session days", 30)
        secure_cookie = prompt_bool("Use secure_cookie", True)

        entered_secret_key = args.secret_key or prompt_string(
            "Secret key (leave empty to auto-generate)", ""
        )
        secret_key = entered_secret_key or secrets.token_urlsafe(48)
    except ValueError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 2

    password_hash = generate_password_hash(password)

    print("\nPaste this into auth.yml:\n")
    print("auth:")
    print(f"  username: {yaml_quote(username)}")
    print(f"  password_hash: {yaml_quote(password_hash)}")
    print(f"  secret_key: {yaml_quote(secret_key)}")
    print(f"  session_days: {session_days}")
    print(f"  secure_cookie: {'true' if secure_cookie else 'false'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
