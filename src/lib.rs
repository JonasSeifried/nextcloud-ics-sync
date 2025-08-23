use std::collections::HashSet;

use anyhow::Result;

use icalendar::Calendar;
use log::{debug, info, warn};
use reqwest::Client;

pub mod calendar;
pub mod config;
pub mod nextcloud;

pub async fn sync_calendar(
    client: &Client,
    nextcloud_username: &str,
    nextcloud_password: &str,
    nextcloud_url: &str,
    nextcloud_calendar_url: &str,
    source_calendar: Calendar,
    nextcloud_calendar: Calendar,
) -> Result<()> {
    // 1. Extract events from both calendars into hashmaps for easy lookup.
    let source_events = calendar::extract_events(source_calendar, true);
    let nextcloud_events = calendar::extract_events(nextcloud_calendar, false);

    // 2. Determine which events to create/update and which to delete.
    let mut events_to_upload = Vec::new();
    let mut uids_to_delete: HashSet<String> = nextcloud_events.keys().cloned().collect();

    debug!("Calculating sync diff...");
    for (uid, source_event) in source_events {
        let b = uids_to_delete.remove(&uid);
        if !b {
            warn!("Failed to delete entry for UID: {}", uid);
        } else {
            info!("Deleted entry for UID: {}", uid);
        }

        if let Some(existing_event) = nextcloud_events.get(&uid) {
            if calendar::should_skip(&source_event, existing_event) {
                info!("Skipping unchanged event with UID: {}", uid);
                continue;
            }
        }
        events_to_upload.push(source_event);
    }

    if !events_to_upload.is_empty() {
        info!(
            "Uploading {} new/modified events...",
            events_to_upload.len()
        );
        calendar::handle_uploads(
            client,
            nextcloud_username,
            nextcloud_password,
            nextcloud_calendar_url,
            events_to_upload,
        )
        .await?;
    } else {
        info!("No new or modified events to upload.");
    }

    debug!("Remaining UIDs to delete: {:?}", uids_to_delete);

    calendar::handle_deletes(
        client,
        nextcloud_username,
        nextcloud_password,
        nextcloud_url,
        nextcloud_calendar_url,
        uids_to_delete,
    )
    .await?;

    info!("Calendar sync complete. ✅");
    Ok(())
}
