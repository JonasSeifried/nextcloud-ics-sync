use anyhow::{Context, Result};
use dotenv::dotenv;
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
    pub fetch_calendars: Option<bool>,
}

fn get_optional_fetch_config() -> Result<Option<bool>> {
    match env::var("FETCH_CALENDARS") {
        core::result::Result::Ok(val_str) => {
            let value = val_str.parse::<bool>().context(format!(
                "'FETCH_CALENDARS' is invalid: could not parse '{}' as a boolean",
                val_str
            ))?;
            Ok(Some(value))
        }
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(e) => Err(e).context("'FETCH_CALENDARS' contained invalid unicode"),
    }
}

impl Config {
    // Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        dotenv().ok();
        let nextcloud_url =
            env::var("NEXTCLOUD_URL").context("NEXTCLOUD_URL environment variable not set")?;
        let nextcloud_username = env::var("NEXTCLOUD_USERNAME")
            .context("NEXTCLOUD_USERNAME environment variable not set")?;

        let fetch_calendars = get_optional_fetch_config()?;

        let calendar_id = env::var("CALENDAR_ID").context("CALENDAR_ID not set")?;

        Ok(Self {
            ics_url: env::var("ICS_URL").context("ICS_URL environment variable not set")?,
            ics_username: env::var("ICS_USERNAME").ok(),
            ics_password: env::var("ICS_PASSWORD").ok(),
            nextcloud_url: nextcloud_url.clone(),
            nextcloud_calendar_url: format!(
                "{}/remote.php/dav/calendars/{}/{}/",
                nextcloud_url, nextcloud_username, calendar_id
            ),
            nextcloud_username: nextcloud_username,
            nextcloud_password: env::var("NEXTCLOUD_PASSWORD")
                .context("NEXTCLOUD_PASSWORD environment variable not set")?,
            // calendar_id: calendar_id,
            fetch_calendars: fetch_calendars,
        })
    }
}
