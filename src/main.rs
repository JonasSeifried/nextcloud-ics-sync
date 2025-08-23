use anyhow::{Context, Ok, Result};
use dotenv::dotenv;
use log::{debug, info};
use nextcloud_ics_sync::{calendar, config::Config, nextcloud, sync_calendar};
use reqwest::Client;

// TODO: keep custom evens
// TODO: skip event if last_moified didnt change
// TODO: paralelize request
// TODO: Merge Calenders (internal and external)

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    let config = Config::from_env()?;
    env_logger::init();

    let client = Client::new();
    if config.fetch_calendars.unwrap_or(false) {
        let available_calendars = nextcloud::get_calendar_ids(
            &client,
            &config.nextcloud_url,
            &config.nextcloud_username,
            &config.nextcloud_password,
        )
        .await?;
        info!("Available Calendars: [{}]", available_calendars.join(","))
    }

    debug!("Starting calendar sync...");

    debug!("Downloading calendar from {}...", config.ics_url);

    let source_calendar = calendar::fetch_and_parse_calendar(
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

    let nextcloud_calendar = calendar::fetch_and_parse_calendar(
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

    sync_calendar(
        &client,
        &config.nextcloud_username,
        &config.nextcloud_password,
        &config.nextcloud_url,
        &config.nextcloud_calendar_url,
        source_calendar,
        nextcloud_calendar,
    )
    .await
    .with_context(|| "Failed to sync calendars.")?;

    info!("Sync process completed.");
    Ok(())
}
