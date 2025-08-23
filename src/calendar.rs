use std::collections::{HashMap, HashSet};

use crate::nextcloud;

use anyhow::{Context, Result, anyhow, bail};
use futures::future::try_join_all;
use icalendar::{Calendar, CalendarComponent, Component, Event};
use log::{debug, info};
use reqwest::{Client, StatusCode};
use urlencoding::encode;

pub async fn fetch_and_parse_calendar(
    client: &Client,
    url: &str,
    username: Option<String>,
    password: Option<String>,
) -> Result<Calendar> {
    let mut request_builder = client.get(url);

    if let Some(ics_username) = username {
        request_builder = request_builder.basic_auth(ics_username, password);
    }

    let response = request_builder
        .send()
        .await
        .with_context(|| format!("Failed to download ICS file. URL: {}", url))?;

    if !response.status().is_success() {
        bail!(
            "Failed to download ICS file. Status code: {} URL: {}",
            response.status(),
            url
        );
    }

    let ics_content = response
        .bytes()
        .await
        .with_context(|| format!("Failed to read ICS content. URL: {}", url))?;

    let ics_text = str::from_utf8(&ics_content)
        .with_context(|| format!("Invalid UTF-8 in ICS content. URL: {}", url))?;

    ics_text
        .parse::<Calendar>()
        .map_err(|e: String| (anyhow!(e)))
        .with_context(|| format!("Failed to parse iCalendar content. URL: {}", url))
}

fn process_event(mut event: Event) -> Event {
    if let Some(uid) = event.get_uid() {
        let encoded_uid = encode(uid).into_owned().replace("%2F", "-");
        event.uid(&encoded_uid);
    }
    event
}

pub fn extract_events(calendar: Calendar, process_events: bool) -> HashMap<String, Event> {
    calendar
        .components
        .into_iter()
        .filter_map(|component| {
            if let CalendarComponent::Event(event) = component {
                let event = if process_events {
                    process_event(event)
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

pub fn should_skip(source_event: &Event, existing_event: &Event) -> bool {
    match (
        source_event.get_last_modified(),
        existing_event.get_last_modified(),
    ) {
        (Some(source_ts), Some(existing_ts)) => source_ts == existing_ts,
        _ => false,
    }
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
            let uid = &event.get_uid().unwrap_or_default();
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
                .context(format!("Failed to upload event with UID: {}", uid))?;

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
    nextcloud_url: &str,
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
        let nextcloud_url = nextcloud_url.to_string();
        let nextcloud_calendar_url = nextcloud_calendar_url.to_string();

        tokio::spawn(async move {
            let href: String = nextcloud::get_href_by_uid(
                &client,
                &nextcloud_calendar_url,
                &username,
                &password,
                &uid,
            )
            .await?;
            let delete_url = format!("{}{}", nextcloud_url, href);

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
    Ok(())
}
