use anyhow::{Context, Ok, Result, anyhow, bail};
use dotenv::dotenv;
use futures::future::try_join_all;
use icalendar::{Calendar, CalendarComponent, Component, Event};
use log::{debug, error, info, warn};
use reqwest::{Client, StatusCode};
use serde_xml_rs::from_str;
use std::{
    collections::{HashMap, HashSet},
    env,
};
use urlencoding::encode;

use serde::{Deserialize, Serialize};

// TODO: keep custom evens
// TODO: skip event if last_moified didnt change
// TODO: paralelize request
// TODO: Merge Calenders (internal and external)

#[derive(Debug)]
struct Config {
    ics_url: String,
    ics_username: Option<String>,
    ics_password: Option<String>,
    nextcloud_url: String,
    nextcloud_calendar_url: String,
    nextcloud_username: String,
    nextcloud_password: String,
    // calendar_id: String,
    fetch_calendars: Option<bool>,
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
    fn from_env() -> Result<Self> {
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

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename = "multistatus")]
pub struct Multistatus {
    #[serde(rename = "d:response", default)]
    pub responses: Vec<Response>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename = "d:response")]
pub struct Response {
    #[serde(rename = "d:href")]
    pub href: String,

    #[serde(rename = "d:propstat", default)]
    pub propstats: Vec<Propstat>,
}

// --------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename = "d:propstat")]
pub struct Propstat {
    #[serde(rename = "d:prop")]
    pub prop: Prop,

    #[serde(rename = "d:status")]
    pub status: String,
}

// --------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename = "d:prop")]
pub struct Prop {
    #[serde(rename = "d:resourcetype", default)]
    pub resourcetype: Option<ResourceType>,

    #[serde(rename = "d:owner", default)]
    pub owner: Option<Owner>,

    #[serde(rename = "d:displayname", default)]
    pub displayname: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename = "d:resourcetype")]
pub struct ResourceType {
    #[serde(rename = "x1:deleted-calendar")]
    pub calendar_deleted: Option<Empty>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename = "d:owner")]
pub struct Owner {
    #[serde(rename = "d:href")]
    pub href: String,
}

// A simple unit struct to handle empty tags like `<d:calendar_deleted/>`
#[derive(Debug, Deserialize, Serialize)]
pub struct Empty;

fn get_calendar_id_after_username<'a>(s: &'a str, username: &str) -> Option<String> {
    s.split_once(&format!("/{}/", username))
        .map(|(_, remainder)| remainder.trim_matches('/').to_string())
        .filter(|remainder| !remainder.is_empty())
}

async fn get_calendar_ids(
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
        .expect("Failed to send request");

    let xml_data = response
        .text()
        .await
        .expect("Failed to read PROPFIND response body");

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
        .filter_map(|r| get_calendar_id_after_username(&r.href, username))
        .collect();
    Ok(ids)
}

async fn get_href_by_uid(
    client: &Client,
    nextcloud_calendar_url: &str,
    username: &str,
    password: &str,
    uid: &str,
) -> Result<String> {
    let report_body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
         <c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
           <d:prop>
             <d:getetag/>
             <c:calendar-data/>
           </d:prop>
           <c:filter>
             <c:comp-filter name="VCALENDAR">
               <c:comp-filter name="VEVENT">
                 <c:prop-filter name="UID">
                   <c:text-match collation="i;unicode-casemap" match-type="equals">{}</c:text-match>
                 </c:prop-filter>
               </c:comp-filter>
             </c:comp-filter>
           </c:filter>
         </c:calendar-query>"#,
        uid
    );

    let response = client
        .request(
            reqwest::Method::from_bytes(b"REPORT").unwrap(),
            nextcloud_calendar_url,
        )
        .basic_auth(username, Some(password))
        .header("Depth", "1")
        .header("Content-Type", "application/xml")
        .body(report_body)
        .send()
        .await
        .context("Failed to send request")?;

    let xml_data = response
        .text()
        .await
        .context("Failed to read PROPFIND response body")?;

    let multistatus = from_str::<Multistatus>(&xml_data)
        .with_context(|| format!("Failed to parse XML for UID: {} \n XML: {}", uid, xml_data))?;
    multistatus
        .responses
        .first()
        .map(|r| r.href.clone())
        .with_context(|| format!("Failed to find href for UID: {}", uid))
}

async fn fetch_and_parse_calendar(
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

fn extract_events(calendar: Calendar, process_events: bool) -> HashMap<String, Event> {
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

fn should_skip(source_event: &Event, existing_event: &Event) -> bool {
    match (
        source_event.get_last_modified(),
        existing_event.get_last_modified(),
    ) {
        (Some(source_ts), Some(existing_ts)) => source_ts == existing_ts,
        _ => false,
    }
}

/// Handles the concurrent upload of multiple events to Nextcloud.
async fn handle_uploads(
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

            debug!("Uploading event with UID: {}", uid);

            let request = client
                            .put(&upload_url)
                .basic_auth(&username, Some(&password))
                .header("Content-Type", "text/calendar")
                .body(event_content.clone())
                .build()?;

            let response = client.execute(request)
                .await
                .context(format!(
                    "Failed to upload event with UID: {}", uid))?;

            match response.status() {
                StatusCode::OK | StatusCode::CREATED | StatusCode::NO_CONTENT => {
                    info!("-> Upload successful for UID: {}", uid);
                    Ok(())
                }
                _ => {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    error!(
                        "-> Failed to upload event with UID: {}. Status: {} \n URL: {} \n event body: {}",
                        uid, status, upload_url, event_content
                    );
                    error!("-> Response body: \n {}", body);
                    Err(anyhow::anyhow!(
                        "Upload failed for UID {} with status {}",
                        uid,
                        status
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
async fn handle_deletes(
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
            let href: String =
                get_href_by_uid(&client, &nextcloud_calendar_url, &username, &password, &uid)
                    .await?;
            let delete_url = format!("{}{}", nextcloud_url, href);

            debug!("Deleting event with UID: {}", uid);
            let response = client
                .delete(&delete_url)
                .basic_auth(&username, Some(&password))
                .send()
                .await
                .context(format!("Failed to delete event with UID: {}", uid))?;

            match response.status() {
                StatusCode::OK | StatusCode::NO_CONTENT => {
                    info!("-> Deletion successful for UID: {}", uid);
                    Ok(())
                }
                _ => {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    error!(
                        "-> Failed to delete event with UID: {}. Status: {}",
                        uid, status
                    );
                    error!("-> Response body: {}", body);
                    Err(anyhow::anyhow!(
                        "Deletion failed for UID {} with status {}",
                        uid,
                        status
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
/// Synchronizes events from a source calendar to a Nextcloud calendar.
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
    let source_events = extract_events(source_calendar, true);
    let nextcloud_events = extract_events(nextcloud_calendar, false);

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
            if should_skip(&source_event, existing_event) {
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
        handle_uploads(
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

    handle_deletes(
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

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    let config = Config::from_env()?;
    env_logger::init();

    let client = Client::new();
    if config.fetch_calendars.unwrap_or(false) {
        let available_calendars = get_calendar_ids(
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

    let source_calendar = fetch_and_parse_calendar(
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

    let nextcloud_calendar = fetch_and_parse_calendar(
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
