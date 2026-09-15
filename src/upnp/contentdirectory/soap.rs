//! The SOAP surface of ContentDirectory.

use quick_xml::Reader;
use quick_xml::events::Event;

use super::{BrowseResponse, Fault};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowseFlag {
    Metadata,
    DirectChildren,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowseRequest {
    pub object_id: String,
    pub browse_flag: BrowseFlag,
    pub starting_index: usize,
    /// Zero means every child.
    pub requested_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchRequest {
    pub container_id: String,
    pub criteria: String,
    pub starting_index: usize,
    pub requested_count: usize,
}

pub fn parse_browse(body: &str) -> Result<BrowseRequest, Fault> {
    let object_id = element_text(body, "ObjectID").ok_or(Fault::INVALID_ARGS)?;
    let flag = element_text(body, "BrowseFlag").ok_or(Fault::INVALID_ARGS)?;
    let browse_flag = match flag.as_str() {
        "BrowseMetadata" => BrowseFlag::Metadata,
        "BrowseDirectChildren" => BrowseFlag::DirectChildren,
        _ => return Err(Fault::INVALID_ARGS),
    };
    Ok(BrowseRequest {
        object_id,
        browse_flag,
        starting_index: element_text(body, "StartingIndex")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0),
        requested_count: element_text(body, "RequestedCount")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0),
    })
}

pub fn parse_search(body: &str) -> Result<SearchRequest, Fault> {
    Ok(SearchRequest {
        container_id: element_text(body, "ContainerID").ok_or(Fault::INVALID_ARGS)?,
        criteria: element_text(body, "SearchCriteria").ok_or(Fault::INVALID_ARGS)?,
        starting_index: element_text(body, "StartingIndex")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0),
        requested_count: element_text(body, "RequestedCount")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0),
    })
}

pub fn browse_envelope(response: &BrowseResponse) -> String {
    result_envelope("Browse", response)
}

pub fn search_envelope(response: &BrowseResponse) -> String {
    result_envelope("Search", response)
}

/// A string replace over the finished document would also rewrite a track title.
fn result_envelope(action: &str, response: &BrowseResponse) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{action}Response xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1"><Result>{result}</Result><NumberReturned>{returned}</NumberReturned><TotalMatches>{total}</TotalMatches><UpdateID>{update}</UpdateID></u:{action}Response></s:Body></s:Envelope>"#,
        result = escape_text(&response.result),
        returned = response.number_returned,
        total = response.total_matches,
        update = response.update_id,
    )
}

pub fn simple_envelope(action: &str, service: &str, argument: &str, value: &str) -> String {
    arguments_envelope(action, service, &[(argument, value.to_owned())])
}

/// Out-arguments go in the order the service definition declares.
pub fn arguments_envelope(action: &str, service: &str, arguments: &[(&str, String)]) -> String {
    let body: String = arguments
        .iter()
        .map(|(name, value)| format!("<{name}>{}</{name}>", escape_text(value)))
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{action}Response xmlns:u="urn:schemas-upnp-org:service:{service}:1">{body}</u:{action}Response></s:Body></s:Envelope>"#
    )
}

pub fn argument(body: &str, name: &str) -> Option<String> {
    element_text(body, name)
}

pub fn fault_envelope(fault: &Fault) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><s:Fault><faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring><detail><UPnPError xmlns="urn:schemas-upnp-org:control-1-0"><errorCode>{code}</errorCode><errorDescription>{description}</errorDescription></UPnPError></detail></s:Fault></s:Body></s:Envelope>"#,
        code = fault.code,
        description = fault.description,
    )
}

fn escape_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn element_text(xml: &str, local_name: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut inside = false;
    let mut text = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                if local(e.name().as_ref()) == local_name {
                    inside = true;
                    text.clear();
                }
            }
            Ok(Event::Text(e)) if inside => {
                let raw = e.into_inner();
                match quick_xml::escape::unescape(&raw) {
                    Ok(unescaped) => text.push_str(&unescaped),
                    Err(_) => text.push_str(&raw),
                }
            }
            Ok(Event::GeneralRef(e)) if inside => {
                let reference = e.into_inner();
                match entity(&reference) {
                    Some(character) => text.push(character),
                    None => {
                        tracing::warn!(%reference, "dropping an entity reference nothing defines")
                    }
                }
            }
            Ok(Event::End(e)) if inside => {
                if local(e.name().as_ref()) == local_name {
                    return Some(text.trim().to_owned());
                }
            }
            Ok(Event::Empty(e)) => {
                if local(e.name().as_ref()) == local_name {
                    return Some(String::new());
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

fn entity(name: &str) -> Option<char> {
    match name {
        "quot" => Some('"'),
        "apos" => Some('\''),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        numeric => {
            let digits = numeric.strip_prefix('#')?;
            let code = match digits.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => digits.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

fn local(name: &str) -> &str {
    match name.rfind(':') {
        Some(colon) => &name[colon + 1..],
        None => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upnp::search;

    const REQUEST: &str = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body>
<u:Browse xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1">
<ObjectID>0</ObjectID><BrowseFlag>BrowseDirectChildren</BrowseFlag><Filter>*</Filter>
<StartingIndex>0</StartingIndex><RequestedCount>10</RequestedCount><SortCriteria></SortCriteria>
</u:Browse></s:Body></s:Envelope>"#;

    #[test]
    fn a_browse_request_is_read_whatever_the_namespace_prefix() {
        let parsed = parse_browse(REQUEST).unwrap();
        assert_eq!(parsed.object_id, "0");
        assert_eq!(parsed.browse_flag, BrowseFlag::DirectChildren);
        assert_eq!(parsed.starting_index, 0);
        assert_eq!(parsed.requested_count, 10);
    }

    #[test]
    fn an_argument_carrying_escaped_quotes_survives_being_read() {
        let body = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body>
<u:Search xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1">
<ContainerID>0</ContainerID>
<SearchCriteria>upnp:class derivedfrom &quot;object.container.album&quot; and @refID exists false</SearchCriteria>
<Filter>*</Filter><StartingIndex>0</StartingIndex><RequestedCount>0</RequestedCount><SortCriteria></SortCriteria>
</u:Search></s:Body></s:Envelope>"#;
        let parsed = parse_search(body).expect("it parses");
        assert_eq!(
            parsed.criteria,
            r#"upnp:class derivedfrom "object.container.album" and @refID exists false"#
        );
        assert!(search::parse(&parsed.criteria).is_ok());
    }

    #[test]
    fn an_unknown_browse_flag_is_refused_rather_than_guessed() {
        let bad = REQUEST.replace("BrowseDirectChildren", "BrowseEverything");
        assert_eq!(parse_browse(&bad), Err(Fault::INVALID_ARGS));
    }

    #[test]
    fn the_didl_is_escaped_into_the_result_element() {
        let response = BrowseResponse {
            result: r#"<DIDL-Lite><item id="a"/></DIDL-Lite>"#.to_owned(),
            number_returned: 1,
            total_matches: 7,
            update_id: 3,
        };
        let envelope = browse_envelope(&response);
        assert!(envelope.contains("&lt;DIDL-Lite&gt;"));
        assert!(envelope.contains("<TotalMatches>7</TotalMatches>"));
        assert!(envelope.contains("<NumberReturned>1</NumberReturned>"));
    }

    #[test]
    fn a_fault_carries_a_code_and_a_description() {
        let xml = fault_envelope(&Fault::NO_SUCH_OBJECT);
        assert!(xml.contains("<errorCode>701</errorCode>"));
        assert!(xml.contains("<errorDescription>No such object</errorDescription>"));
    }
}
