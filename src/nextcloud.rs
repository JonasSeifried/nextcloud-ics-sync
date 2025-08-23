use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_xml_rs::from_str;

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

#[derive(Debug, Deserialize, Serialize)]
pub struct Empty;

fn get_calendar_id_after_username<'a>(s: &'a str, username: &str) -> Option<String> {
    s.split_once(&format!("/{}/", username))
        .map(|(_, remainder)| remainder.trim_matches('/').to_string())
        .filter(|remainder| !remainder.is_empty())
}

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

pub async fn get_href_by_uid(
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
