use dotenv::dotenv;
use icalendar::{Calendar, CalendarComponent, Component, parser::read_calendar};
use log::{debug, error, info};
use reqwest::{Client, StatusCode};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    env_logger::init();
    // Ensure these variables are set in your environment or a `.env` file.
    let ics_url = env::var("ICS_URL")?;
    let ics_username = env::var("ICS_USERNAME").ok();
    let ics_password = env::var("ICS_PASSWORD").ok();
    let nextcloud_url = env::var("NEXTCLOUD_URL")?;
    let nextcloud_username = env::var("NEXTCLOUD_USERNAME")?;
    let nextcloud_password = env::var("NEXTCLOUD_PASSWORD")?;
    let calendar_name = env::var("CALENDAR_NAME")?;

    debug!("Starting calendar sync...");

    debug!("Downloading calendar from {}...", ics_url);
    let client = Client::new();

    let mut request_builder = client.get(&ics_url);

    if let Some(ics_username) = ics_username {
        request_builder = request_builder.basic_auth(ics_username, ics_password);
    }

    let response = request_builder.send().await?;

    if !response.status().is_success() {
        error!(
            "Failed to download ICS file. Status code: {}",
            response.status()
        );
        return Err("Download failed.".into());
    }

    let ics_content = response.bytes().await?;

    let ics_text = str::from_utf8(&ics_content).expect("Invalid UTF-8 in ICS content");

    let calendar: Calendar = ics_text.parse().expect("Failed to parse iCalendar content");

    let base_upload_url = format!(
        "{}/remote.php/dav/calendars/{}/{}/",
        nextcloud_url, nextcloud_username, calendar_name
    );

    debug!("Uploading events to calendar {}...", calendar_name);

    for component in &calendar.components {
        if let CalendarComponent::Event(event) = component {
            let uid = event.get_uid().expect("Event is missing a UID");

            // The filename on the server will be the UID with a .ics extension.
            let upload_url = format!("{}{}.ics", base_upload_url, uid);

            debug!(
                "Attempting to upload event '{}' with UID: {}",
                event.get_summary().unwrap_or("No summary"),
                uid
            );

            let mut event_calendar = Calendar::new();
            event_calendar.push(event.to_owned());
            let event_content = event_calendar.to_string();
            // Serialize the single event component back into an ICS string.
            // let event_content = event.to_string();

            // Perform the PUT request to upload this single event.
            let response = client
                .put(&upload_url)
                .basic_auth(&nextcloud_username, Some(&nextcloud_password))
                .header("Content-Type", "text/calendar")
                .body(event_content)
                .send()
                .await?;

            // Check the status of the upload response for this event.
            match response.status() {
                StatusCode::OK | StatusCode::CREATED | StatusCode::NO_CONTENT => {
                    debug!("  -> Upload successful for UID: {}", uid);
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
    }

    info!("Sync process completed.");
    Ok(())
}
