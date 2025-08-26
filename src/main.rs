use anyhow::{Context, Ok, Result};
use clap::{Parser, Subcommand, command};
use dotenv::dotenv;
use log::info;
use nextcloud_ics_sync::{
    config::{self, Config},
    ics_parser, nextcloud, sync_calendar,
};
use reqwest::Client;

// TODO: Merge Calenders (internal and external)

/// Simple program to sync an ICS calendar to a Nextcloud calendar.
#[derive(Parser, Debug)]
#[command(version, about = "A calendar synchronization tool", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Synchronizes events with the calendar provider [DEFAULT]
    Sync,
    /// Fetch available calendar ids (alias `fetch`)
    #[clap(alias = "fetch")]
    FetchCalendars,
    /// Delete all synced events (alias `delete`)
    #[clap(alias = "delete")]
    DeleteSyncedEvents,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    env_logger::init();

    let cli = Cli::parse();
    let client = Client::new();

    match cli.command {
        Some(Commands::FetchCalendars) => print_available_calendar_ids(&client).await,
        Some(Commands::DeleteSyncedEvents) => delete_synced_events(&client).await,
        None | Some(Commands::Sync) => sync_calendars(&client).await,
    }
}

async fn delete_synced_events(client: &Client) -> Result<()> {
    let config = Config::from_env()?;

    let nextcloud_calendar = get_nextcloud_calendar(client, &config).await?;

    nextcloud_ics_sync::delete_synced_events(
        client,
        nextcloud_calendar,
        &config.nextcloud_calendar_url,
        &config.nextcloud_username,
        &config.nextcloud_password,
    )
    .await
}

async fn sync_calendars(client: &Client) -> Result<()> {
    let config = Config::from_env()?;

    info!("Downloading source calendar from {}...", config.ics_url);

    let source_calendar = ics_parser::fetch_and_parse_calendar(
        &client,
        &config.ics_url,
        config.ics_username.clone(),
        config.ics_password.clone(),
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

    let nextcloud_calendar = get_nextcloud_calendar(client, &config).await?;

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

async fn print_available_calendar_ids(client: &Client) -> Result<()> {
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
    );
    Ok(())
}

async fn get_nextcloud_calendar(client: &Client, config: &Config) -> Result<icalendar::Calendar> {
    ics_parser::fetch_and_parse_calendar(
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
    })
}
