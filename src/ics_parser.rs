use anyhow::{Context, Result, anyhow, bail};
use icalendar::Calendar;
use reqwest::{Client, Response};

async fn fetch_ics_data(
    client: &Client,
    url: &str,
    username: Option<String>,
    password: Option<String>,
) -> Result<Response> {
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

    Ok(response)
}

pub async fn fetch_and_parse_calendar(
    client: &Client,
    url: &str,
    username: Option<String>,
    password: Option<String>,
) -> Result<Calendar> {
    let response = fetch_ics_data(client, url, username, password).await?;

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
