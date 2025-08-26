use std::collections::{HashMap, HashSet};

use anyhow::{Context, Ok, Result};

use icalendar::{Calendar, Component, Event};
use log::{debug, info};
use reqwest::Client;

pub mod config;
pub mod ics_parser;
pub mod nextcloud;

fn get_synced_uids(events: &HashMap<String, Event>) -> HashSet<String> {
    events
        .iter()
        .filter(|(_, event)| event.property_value("X-SYNCED").is_some())
        .map(|(uid, _)| uid.clone())
        .collect()
}

fn calculate_diff<'a>(
    source_events: &'a HashMap<String, Event>,
    nextcloud_events: &HashMap<String, Event>,
) -> (Vec<&'a Event>, HashSet<String>) {
    let mut events_to_upload = Vec::new();
    let mut uids_to_delete: HashSet<String> = get_synced_uids(nextcloud_events);

    debug!("Calculating sync diff...");
    for (uid, source_event) in source_events {
        uids_to_delete.remove(uid);

        if let Some(existing_event) = nextcloud_events.get(uid) {
            if nextcloud::api::should_skip(source_event, existing_event) {
                debug!("Skipping unchanged event with UID: {}", uid);
                continue;
            }
        }
        events_to_upload.push(source_event);
    }
    (events_to_upload, uids_to_delete)
}

pub async fn sync_calendar(
    client: &Client,
    nextcloud_username: &str,
    nextcloud_password: &str,
    nextcloud_calendar_url: &str,
    source_calendar: Calendar,
    nextcloud_calendar: Calendar,
) -> Result<()> {
    let source_events = nextcloud::api::extract_events(source_calendar, true);
    let nextcloud_events = nextcloud::api::extract_events(nextcloud_calendar, false);

    let (events_to_upload, uids_to_delete) = calculate_diff(&source_events, &nextcloud_events);

    if !events_to_upload.is_empty() {
        info!(
            "Uploading {} new/modified events...",
            events_to_upload.len()
        );

        let owned_events_to_upload = events_to_upload.clone().into_iter().cloned().collect();
        nextcloud::api::handle_uploads(
            client,
            nextcloud_username,
            nextcloud_password,
            nextcloud_calendar_url,
            owned_events_to_upload,
        )
        .await
        .context("Failed to upload events")?;
    } else {
        info!("No new or modified events to upload.");
    }

    if !uids_to_delete.is_empty() {
        info!("Deleting {} stale events...", uids_to_delete.len());
        nextcloud::api::handle_deletes(
            client,
            nextcloud_username,
            nextcloud_password,
            nextcloud_calendar_url,
            uids_to_delete,
        )
        .await
        .context("Failed to delete events")?;
    } else {
        info!("No stale events to delete.");
    }

    info!("Calendar sync complete. ✅");
    Ok(())
}

pub async fn delete_synced_events(
    client: &Client,
    nextcloud_calendar: Calendar,
    nextcloud_calendar_url: &str,
    username: &str,
    password: &str,
) -> Result<()> {
    info!("Deleting all synced events...");

    let nextcloud_events = nextcloud::api::extract_events(nextcloud_calendar, false);
    let uids_to_delete: HashSet<String> = get_synced_uids(&nextcloud_events);

    nextcloud::api::handle_deletes(
        client,
        username,
        password,
        nextcloud_calendar_url,
        uids_to_delete,
    )
    .await
}
