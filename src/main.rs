use anyhow::{Context, Ok, Result, anyhow, bail};
use dotenv::dotenv;
use icalendar::{Calendar, CalendarComponent, Component, Event};
use log::{debug, error, info, warn};
use reqwest::{Client, StatusCode};
use serde_xml_rs::from_str;
use std::{collections::HashMap, env};

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
    nextcloud_username: String,
    nextcloud_password: String,
    calendar_id: Result<String>,
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
        Ok(Self {
            ics_url: env::var("ICS_URL").context("ICS_URL environment variable not set")?,
            ics_username: env::var("ICS_USERNAME").ok(),
            ics_password: env::var("ICS_PASSWORD").ok(),
            nextcloud_url: env::var("NEXTCLOUD_URL")
                .context("NEXTCLOUD_URL environment variable not set")?,
            nextcloud_username: env::var("NEXTCLOUD_USERNAME")
                .context("NEXTCLOUD_USERNAME environment variable not set")?,
            nextcloud_password: env::var("NEXTCLOUD_PASSWORD")
                .context("NEXTCLOUD_PASSWORD environment variable not set")?,
            calendar_id: env::var("CALENDAR_ID").context("CALENDAR_ID not set"),
            fetch_calendars: get_optional_fetch_config()?,
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename = "multistatus")]
pub struct Multistatus {
    #[serde(rename = "d:response")]
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

async fn sync_calendar(
    client: &Client,
    nextcloud_username: &str,
    nextcloud_password: &str,
    nextcloud_calendar_base_url: &str,
    source_calendar: Calendar,
    nextcloud_calendar: Calendar,
) -> Result<()> {
    let mut source_events: HashMap<String, Event> = HashMap::new();
    for component in source_calendar.components {
        if let CalendarComponent::Event(event) = component {
            if let Some(uid) = event.get_uid() {
                source_events.insert(uid.replace("/", "%25"), event);
            }
        }
    }

    let mut nexcloud_events: HashMap<String, Event> = HashMap::new();
    for component in nextcloud_calendar.components {
        if let CalendarComponent::Event(event) = component {
            if let Some(uid) = event.get_uid() {
                nexcloud_events.insert(uid.replace("/", "%25"), event);
            }
        }
    }

    let mut uids_to_delete: Vec<String> = nexcloud_events.keys().cloned().collect();

    let mut join_handles = Vec::new();

    info!("Starting sync of events...");
    for (uid, event) in source_events {
        let uid = uid.replace("/", "%25");

        uids_to_delete.retain(|x| x.eq(&uid));

        if let (Some(existing_event), Some(event_last_modified)) =
            (nexcloud_events.get(&uid), event.get_last_modified())
        {
            if existing_event
                .get_last_modified()
                .map(|existing_last_modified| existing_last_modified == event_last_modified)
                .unwrap_or(false)
            {
                info!("Skipping event with UID: {}", uid);
                continue;
            }
        }

        let event_calendar = Calendar::new().push(event).done();

        let upload_url = format!("{}{}.ics", nextcloud_calendar_base_url, uid);
        let event_content = event_calendar.to_string();

        debug!("Attempting to upload event with UID: {}", uid);

        let client = client.clone();
        let uid = uid.clone();
        let upload_url = upload_url.clone();
        let nextcloud_username = nextcloud_username.to_string();
        let nextcloud_password = nextcloud_password.to_string();

        let future = async move {
            let response = client
                .put(&upload_url)
                .basic_auth(nextcloud_username, Some(nextcloud_password))
                .header("Content-Type", "text/calendar")
                .body(event_content)
                .send()
                .await
                .context("Failed to upload event.")?;

            match response.status() {
                StatusCode::OK | StatusCode::CREATED | StatusCode::NO_CONTENT => {
                    info!("  -> Upload successful for UID: {}", uid);
                }
                _ => {
                    error!(
                        "  -> Failed to upload event with UID: {}. Status code: {}",
                        uid,
                        response.status()
                    );
                    error!(
                        "  -> Response body: {:?}",
                        response.text().await.unwrap_or_default()
                    );
                }
            }

            Ok(())
        };

        let handle = tokio::spawn(future);
        join_handles.push(handle);
    }

    for handle in join_handles {
        handle.await??;
    }

    info!("Checking for events to delete...");
    for uid in uids_to_delete {
        let uid = uid.replace("/", "%25");
        let delete_url = format!("{}{}.ics", nextcloud_calendar_base_url, uid);
        info!("Attempting to delete event with UID: {}", uid);
        let response = client
            .delete(&delete_url)
            .basic_auth(nextcloud_username, Some(nextcloud_password))
            .send()
            .await
            .context("Failed to delete event.")?;

        match response.status() {
            StatusCode::OK | StatusCode::NO_CONTENT => {
                info!("  -> Deletion successful for UID: {}", uid);
            }
            _ => {
                error!(
                    "  -> Failed to delete event with UID: {}. Status code: {}",
                    uid,
                    response.status()
                );
                error!(
                    "  -> Response body: {:?}",
                    response.text().await.unwrap_or_default()
                );
            }
        }
    }
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

    let calendar_id = config.calendar_id?;

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

    let nextcloud_calendar_base_url = format!(
        "{}/remote.php/dav/calendars/{}/{}/",
        config.nextcloud_url, config.nextcloud_username, calendar_id
    );

    let nextcloud_calendar = fetch_and_parse_calendar(
        &client,
        &format!("{}?export", nextcloud_calendar_base_url),
        Some(config.nextcloud_username.clone()),
        Some(config.nextcloud_password.clone()),
    )
    .await
    .with_context(|| {
        format!(
            "Failed to fetch and parse current calendar. URL: {}?export",
            nextcloud_calendar_base_url
        )
    })?;

    sync_calendar(
        &client,
        &config.nextcloud_username,
        &config.nextcloud_password,
        &nextcloud_calendar_base_url,
        source_calendar,
        nextcloud_calendar,
    )
    .await
    .with_context(|| "Failed to sync calendars.")?;

    info!("Sync process completed.");
    Ok(())
}
