use std::{
    io::{self, Write},
    process,
};

use rand::RngCore;
use scrypt::{Params, scrypt};

fn prompt(label: &str, default: Option<&str>) -> Result<String, String> {
    let mut output = io::stdout().lock();
    match default {
        Some(default) => write!(output, "{label} [{default}]: "),
        None => write!(output, "{label}: "),
    }
    .and_then(|()| output.flush())
    .map_err(|error| error.to_string())?;

    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .map_err(|error| error.to_string())?;
    let value = value.trim().to_owned();
    if value.is_empty() {
        Ok(default.unwrap_or_default().to_owned())
    } else {
        Ok(value)
    }
}

fn prompt_yes_no(label: &str, default: bool) -> Result<bool, String> {
    let choice = if default { "Y/n" } else { "y/N" };
    print!("{label} ({choice}): ");
    io::stdout().flush().map_err(|error| error.to_string())?;
    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .map_err(|error| error.to_string())?;
    match value.trim().to_lowercase().as_str() {
        "" => Ok(default),
        "y" | "yes" | "true" | "1" => Ok(true),
        "n" | "no" | "false" | "0" => Ok(false),
        _ => Err(format!("{label} must be yes or no")),
    }
}

fn quote_yaml(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn parse_args() -> Result<(Option<String>, Option<String>), String> {
    let mut password = None;
    let mut secret = None;
    let mut args = std::env::args().skip(2);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--password" => {
                password = Some(
                    args.next()
                        .ok_or_else(|| "--password requires a value".to_owned())?,
                )
            }
            "--secret-key" => {
                secret = Some(
                    args.next()
                        .ok_or_else(|| "--secret-key requires a value".to_owned())?,
                )
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok((password, secret))
}

pub fn run() -> Result<(), String> {
    let (password_arg, secret_arg) = parse_args()?;
    let username = prompt("Username", Some("admin"))?;
    if username.is_empty() {
        return Err("Username cannot be empty".to_owned());
    }

    let password = match password_arg {
        Some(password) if !password.is_empty() => password,
        Some(_) => return Err("Password cannot be empty".to_owned()),
        None => {
            let password = rpassword::prompt_password("Enter new password: ")
                .map_err(|error| error.to_string())?;
            let confirmation = rpassword::prompt_password("Confirm password: ")
                .map_err(|error| error.to_string())?;
            if password != confirmation {
                return Err("Passwords do not match".to_owned());
            }
            if password.is_empty() {
                return Err("Password cannot be empty".to_owned());
            }
            password
        }
    };

    let session_days = prompt("Session days", Some("30"))?
        .parse::<u64>()
        .map_err(|_| "Session days must be a positive integer".to_owned())?;
    if session_days == 0 {
        return Err("Session days must be at least 1".to_owned());
    }
    let secure_cookie = prompt_yes_no("Use secure_cookie", true)?;

    let secret_key = match secret_arg {
        Some(secret) if !secret.is_empty() => secret,
        Some(_) | None => {
            let entered = prompt("Secret key (leave empty to auto-generate)", None)?;
            if entered.is_empty() {
                let mut bytes = [0u8; 48];
                rand::rng().fill_bytes(&mut bytes);
                base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
            } else {
                entered
            }
        }
    };

    let mut salt_bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut salt_bytes);
    let salt = hex::encode(salt_bytes);
    let params = Params::new(15, 8, 1, 64).map_err(|error| error.to_string())?;
    let mut digest = [0u8; 64];
    scrypt(password.as_bytes(), salt.as_bytes(), &params, &mut digest)
        .map_err(|error| error.to_string())?;
    let password_hash = format!("scrypt:32768:8:1${salt}${}", hex::encode(digest));

    println!("\nPaste this into auth.yml:\n");
    println!("auth:");
    println!("  username: {}", quote_yaml(&username));
    println!("  password_hash: {}", quote_yaml(&password_hash));
    println!("  secret_key: {}", quote_yaml(&secret_key));
    println!("  session_days: {session_days}");
    println!("  secure_cookie: {secure_cookie}");
    Ok(())
}

pub fn run_or_exit() {
    if std::env::args()
        .skip(2)
        .any(|argument| argument == "--help" || argument == "-h")
    {
        println!(
            "Interactive auth.yml generator.\n\nUsage: starry-cloud gen-auth [--password PASSWORD] [--secret-key SECRET_KEY]"
        );
        return;
    }
    if let Err(error) = run() {
        eprintln!("Error: {error}");
        process::exit(2);
    }
}
