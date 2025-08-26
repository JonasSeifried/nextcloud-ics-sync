use anyhow::{Context, Result};
use std::env;

#[derive(Debug)]
pub struct Config {
    pub ics_url: String,
    pub ics_username: Option<String>,
    pub ics_password: Option<String>,
    pub nextcloud_url: String,
    pub nextcloud_calendar_url: String,
    pub nextcloud_username: String,
    pub nextcloud_password: String,
    // pub calendar_id: String,
}

impl Config {
    // Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        let nextcloud_url = load_nextcloud_url()?;

        let nextcloud_username = load_nextcloud_username()?;
        let calendar_id = load_calendar_id()?;

        Ok(Self {
            ics_url: load_ics_url()?,
            ics_username: load_ics_username().ok(),
            ics_password: load_ics_password().ok(),
            nextcloud_url: nextcloud_url.clone(),
            nextcloud_calendar_url: format!(
                "{}/remote.php/dav/calendars/{}/{}/",
                nextcloud_url, nextcloud_username, calendar_id
            ),
            nextcloud_username: nextcloud_username,
            nextcloud_password: load_nextcloud_password()?,
            // calendar_id: calendar_id,
        })
    }
}

fn load_env_var(env_var_key: &str) -> Result<String> {
    env::var(env_var_key).with_context(|| format!("{} environment variable not set", env_var_key))
}

pub fn load_ics_url() -> Result<String> {
    load_env_var("ICS_URL")
}

pub fn load_ics_username() -> Result<String> {
    load_env_var("ICS_USERNAME")
}

pub fn load_ics_password() -> Result<String> {
    load_env_var("ICS_PASSWORD")
}

pub fn load_calendar_id() -> Result<String> {
    load_env_var("CALENDAR_ID")
}

pub fn load_nextcloud_username() -> Result<String> {
    load_env_var("NEXTCLOUD_USERNAME")
}

pub fn load_nextcloud_password() -> Result<String> {
    load_env_var("NEXTCLOUD_PASSWORD")
}

pub fn load_nextcloud_url() -> Result<String> {
    load_env_var("NEXTCLOUD_URL")
}
