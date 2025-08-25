use serde::{Deserialize, Serialize};

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
