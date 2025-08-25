use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use reqwest::{Client, StatusCode};
use serde_xml_rs::from_str;

use futures::future::try_join_all;
use icalendar::{Calendar, CalendarComponent, Component, Event};
use log::{debug, info};

use super::{models::Multistatus, utils};

pub async fn get_calendar_ids(
    client: &Client,
    nextcloud_url: &str,
    username: &str,
    password: &str,
) -> Result<Vec<String>> {
    let propfind_body = r#"<?xml version="1.0" encoding="UTF-8"?>
  <d:propfind xmlns:d="DAV:" xmlns:cs="http://calendarserver.org/ns/">
    <d:prop>
      <d:displayname/>
      <cs:getctag/>
      <d:resourcetype/>
      <d:owner/>
    </d:prop>
  </d:propfind>"#;

    let url = format!("{}/remote.php/dav/calendars/{}/", nextcloud_url, username);

    let response = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &url)
        .basic_auth(username, Some(password))
        .header("Depth", "1")
        .header("Content-Type", "application/xml")
        .body(propfind_body)
        .send()
        .await
        .context("Failed to send PROPFIND request to get calendar IDs")?;

    let xml_data = response
        .text()
        .await
        .context("Failed to read PROPFIND response body for calendar IDs")?;

    let multistatus = from_str::<Multistatus>(&xml_data)?;
    let ids = multistatus
        .responses
        .iter()
        .filter(|r| {
            r.propstats.iter().all(|p| {
                p.prop
                    .resourcetype
                    .as_ref()
                    .is_some_and(|t| t.calendar_deleted.is_none())
            })
        })
        .filter_map(|r| utils::get_calendar_id_after_username(&r.href, username))
        .collect();
    Ok(ids)
}

/// Handles the concurrent upload of multiple events to Nextcloud.
pub async fn handle_uploads(
    client: &Client,
    username: &str,
    password: &str,
    base_url: &str,
    events: Vec<Event>,
) -> Result<()> {
    let tasks = events.into_iter().map(|event| {
        let client = client.clone();
        let username = username.to_string();
        let password = password.to_string();
        let base_url = base_url.to_string();

        tokio::spawn(async move {
            let uid = event
                .get_uid()
                .context("Event is missing a UID, cannot upload.")?;
            // URL-encode the UID for the path segment.
            let upload_url = format!("{}{}.ics", base_url, uid);

            let event_calendar = Calendar::new().push(event.clone()).done();
            let event_content = event_calendar.to_string();

            let request = client
                .put(&upload_url)
                .basic_auth(&username, Some(&password))
                .header("Content-Type", "text/calendar")
                .body(event_content.clone())
                .build()?;

            let response = client
                .execute(request)
                .await
                .with_context(|| format!("Failed to upload event with UID: {}", uid))?;

            match response.status() {
                StatusCode::OK | StatusCode::CREATED | StatusCode::NO_CONTENT => {
                    debug!("-> Upload successful for UID: {}", uid);
                    Ok(())
                }
                _ => {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();

                    Err(anyhow::anyhow!(
                        "Upload failed for UID {} with status {} and body of:\n{}",
                        uid,
                        status,
                        body
                    ))
                }
            }
        })
    });

    try_join_all(tasks)
        .await?
        .into_iter()
        .collect::<Result<()>>()?;

    Ok(())
}

/// Handles the concurrent deletion of multiple events from Nextcloud.
pub async fn handle_deletes(
    client: &Client,
    username: &str,
    password: &str,
    nextcloud_calendar_url: &str,
    uids: HashSet<String>,
) -> Result<()> {
    if uids.is_empty() {
        info!("No events to delete.");
        return Ok(());
    }

    info!("Deleting {} events...", uids.len());

    let tasks = uids.into_iter().map(|uid| {
        let client = client.clone();
        let username = username.to_string();
        let password = password.to_string();
        let nextcloud_calendar_url = nextcloud_calendar_url.to_string();

        tokio::spawn(async move {
            let delete_url = format!("{}{}.ics", nextcloud_calendar_url, uid);

            let response = client
                .delete(&delete_url)
                .basic_auth(&username, Some(&password))
                .send()
                .await
                .context(format!("Failed to delete event with UID: {}", uid))?;

            match response.status() {
                StatusCode::OK | StatusCode::NO_CONTENT => {
                    debug!("-> Deletion successful for UID: {}", uid);
                    Ok(())
                }
                _ => {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();

                    Err(anyhow::anyhow!(
                        "Deletion failed for UID {} with status {} and body of:\n{}",
                        uid,
                        status,
                        body
                    ))
                }
            }
        })
    });

    try_join_all(tasks)
        .await?
        .into_iter()
        .collect::<Result<()>>()?;

    info!("Deleted!");

    Ok(())
}

pub fn should_skip(source_event: &Event, existing_event: &Event) -> bool {
    match (
        source_event.get_last_modified(),
        existing_event.get_last_modified(),
    ) {
        (Some(source_ts), Some(existing_ts)) => source_ts == existing_ts,
        _ => false,
    }
}

pub fn extract_events(calendar: Calendar, process_events: bool) -> HashMap<String, Event> {
    calendar
        .components
        .into_iter()
        .filter_map(|component| {
            if let CalendarComponent::Event(event) = component {
                let event = if process_events {
                    utils::process_event(event)
                } else {
                    event
                };
                event.clone().get_uid().map(|uid| (uid.to_string(), event))
            } else {
                None
            }
        })
        .collect()
}
