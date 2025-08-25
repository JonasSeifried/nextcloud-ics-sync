use anyhow::{Context, Ok, Result};
use dotenv::dotenv;
use log::info;
use nextcloud_ics_sync::{
    config::{self, Config},
    ics_parser, nextcloud, sync_calendar,
};
use reqwest::Client;

// TODO: keep custom evens
// TODO: skip event if last_moified didnt change
// TODO: paralelize request
// TODO: Merge Calenders (internal and external)

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    env_logger::init();

    let fetch_calendars = config::load_fetch_calendars()?;

    let client = Client::new();
    if fetch_calendars.unwrap_or(false) {
        let nextcloud_url = config::load_nextcloud_url()?;
        let nextcloud_username = config::load_nextcloud_username()?;
        let nextcloud_password = config::load_nextcloud_password()?;
        let available_calendars = nextcloud::api::get_calendar_ids(
            &client,
            &nextcloud_url,
            &nextcloud_username,
            &nextcloud_password,
        )
        .await?;
        println!(
            "\nAvailable Calendars: [{}]\n",
            available_calendars.join(", ")
        )
    }

    let config = Config::from_env()?;

    info!("Downloading source calendar from {}...", config.ics_url);

    let source_calendar = ics_parser::fetch_and_parse_calendar(
        &client,
        &config.ics_url,
        config.ics_username,
        config.ics_password,
    )
    .await
    .with_context(|| {
        format!(
            "Failed to fetch and parse source calendar. URL: {}",
            config.ics_url
        )
    })?;

    info!(
        "Downloading nextcloud calendar  {}...",
        config.nextcloud_calendar_url
    );

    let nextcloud_calendar = ics_parser::fetch_and_parse_calendar(
        &client,
        &format!("{}?export", &config.nextcloud_calendar_url),
        Some(config.nextcloud_username.clone()),
        Some(config.nextcloud_password.clone()),
    )
    .await
    .with_context(|| {
        format!(
            "Failed to fetch and parse current calendar. URL: {}?export",
            &config.nextcloud_calendar_url
        )
    })?;

    info!("Syncing calendars...");

    sync_calendar(
        &client,
        &config.nextcloud_username,
        &config.nextcloud_password,
        &config.nextcloud_calendar_url,
        source_calendar,
        nextcloud_calendar,
    )
    .await
    .context("Failed to sync calendars.")?;

    info!("Sync process completed.");
    Ok(())
}
