use icalendar::{Component, Event};
use urlencoding::encode;

pub fn get_calendar_id_after_username<'a>(s: &'a str, username: &str) -> Option<String> {
    s.split_once(&format!("/{}/", username))
        .map(|(_, remainder)| remainder.trim_matches('/').to_string())
        .filter(|remainder| !remainder.is_empty())
}

pub fn process_event(mut event: Event) -> Event {
    if let Some(uid) = event.get_uid() {
        let encoded_uid = encode(uid).into_owned().replace("%2F", "-");
        event.uid(&encoded_uid);
        event.add_property("X-SYNCED", "TRUE");
    }
    event
}
