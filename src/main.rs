use anyhow::{Context, Ok, Result, anyhow, bail};
use dotenv::dotenv;
use icalendar::{Calendar, CalendarComponent, Component};
use log::{debug, error, info, warn};
use reqwest::{Client, StatusCode};
use serde_xml_rs::from_str;
use std::{collections::HashMap, env};

use serde::{Deserialize, Serialize};

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

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    env_logger::init();
    // Ensure these variables are set in your environment or a `.env` file.
    let ics_url = env::var("ICS_URL").context("ICS_URL environment variable not set.")?;
    let ics_username = env::var("ICS_USERNAME").ok();
    let ics_password = env::var("ICS_PASSWORD").ok();
    let nextcloud_url = env::var("NEXTCLOUD_URL")?;
    let nextcloud_username = env::var("NEXTCLOUD_USERNAME")
        .context("NEXTCLOUD_USERNAME environment variable not set.")?;
    let nextcloud_password = env::var("NEXTCLOUD_PASSWORD")
        .context("NEXTCLOUD_PASSWORD environment variable not set.")?;

    let fetch_calendars = env::var("FETCH_CALENDARS").ok();

    let client = Client::new();
    if let Some(fetch_calendars) = fetch_calendars
        && fetch_calendars.ne("false")
    {
        let available_calendars = get_calendar_ids(
            &client,
            &nextcloud_url,
            &nextcloud_username,
            &nextcloud_password,
        )
        .await?;
        info!("Available Calendars: [{}]", available_calendars.join(","))
    }

    let calendar_id =
        env::var("CALENDAR_ID").context("CALENDAR_ID environment variable not set.")?;

    debug!("Starting calendar sync...");

    debug!("Downloading calendar from {}...", ics_url);

    let mut request_builder = client.get(&ics_url);

    if let Some(ics_username) = ics_username {
        request_builder = request_builder.basic_auth(ics_username, ics_password);
    }

    let response = request_builder
        .send()
        .await
        .context("Failed to download ICS file.")?;

    if !response.status().is_success() {
        bail!(
            "Failed to download ICS file. Status code: {}",
            response.status()
        );
    }

    let ics_content = response
        .bytes()
        .await
        .context("Failed to read ICS content.")?;

    let ics_text = str::from_utf8(&ics_content).context("Invalid UTF-8 in ICS content")?;

    let source_calendar = ics_text
        .parse::<Calendar>()
        .map_err(|e: String| (anyhow!(e)))
        .context("Failed to parse iCalendar content")?;

    let mut source_uids: HashMap<String, Calendar> = HashMap::new();
    for component in source_calendar.components {
        if let CalendarComponent::Event(event) = component {
            if let Some(uid) = event.get_uid() {
                let mut c = Calendar::new();
                c.push(event.to_owned());
                source_uids.insert(uid.to_string(), c);
            }
        }
    }

    info!("Calendar downloaded successfully.");

    let base_url = format!(
        "{}/remote.php/dav/calendars/{}/{}/",
        nextcloud_url, nextcloud_username, calendar_id
    );

    info!("Nextcloud URL: {}?export", base_url);

    let response2 = client
        .get(format!("{}?export", base_url))
        .basic_auth(&nextcloud_username, Some(&nextcloud_password))
        .send()
        .await
        .context("Failed to download ICS file.")?;

    let ics_content2 = response2
        .bytes()
        .await
        .context("Failed to read ICS content.")?;

    let ics_text2 = str::from_utf8(&ics_content2).expect("Invalid UTF-8 in ICS content");

    let current_calendar: Calendar = ics_text2
        .parse()
        .expect("Failed to parse iCalendar content");

    let mut existing_events: HashMap<String, Calendar> = HashMap::new();
    for component in current_calendar.components {
        if let CalendarComponent::Event(event) = component {
            if let Some(uid) = event.get_uid() {
                if uid == "947c6a8c-ee0b-4643-aa00-78d239202300" {
                    warn!("{}", event.get_uid().unwrap())
                }
                let mut c = Calendar::new();
                c.push(event.to_owned());
                existing_events.insert(uid.to_string(), c);
            }
        }
    }
    let mut uids_to_delete: Vec<String> = existing_events.keys().cloned().collect();

    info!("Starting sync of events...");
    for (uid, event_calendar) in source_uids {
        let uid = uid.replace("/", "%25");

        uids_to_delete.retain(|x| x.eq(&uid));

        let upload_url = format!("{}{}.ics", base_url, uid);
        let event_content = event_calendar.to_string();

        debug!("Attempting to upload event with UID: {}", uid);
        let response = client
            .put(&upload_url)
            .basic_auth(&nextcloud_username, Some(&nextcloud_password))
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
    }

    info!("Checking for events to delete...");
    for uid in uids_to_delete {
        let uid = uid.replace("/", "%25");
        let delete_url = format!("{}{}.ics", base_url, uid);
        info!("Attempting to delete event with UID: {}", uid);
        let response = client
            .delete(&delete_url)
            .basic_auth(&nextcloud_username, Some(&nextcloud_password))
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

    info!("Sync process completed.");
    Ok(())
}
