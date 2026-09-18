//! The three states one process runs, and the one router that serves them.

use std::sync::Arc;

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderMap, Method};
use axum::response::Response;

use crate::api::Control;
use crate::browse::Settings;
use crate::index::Library;
use crate::service::Passes;
use crate::upnp::client::Profiles;
use crate::upnp::description::DeviceIdentity;
use crate::upnp::device::Device;
use crate::upnp::http;

#[derive(Clone)]
pub struct Server {
    pub device: Arc<Device>,
    pub passes: Arc<Passes>,
    pub control: Arc<Control>,
}

impl Server {
    pub fn new(
        library: Library,
        settings: Settings,
        identity: DeviceIdentity,
        clients: Profiles,
    ) -> Self {
        let device = Arc::new(Device::new(library, settings, identity, clients));
        let passes = Arc::new(Passes::default());
        let control = Arc::new(Control::new(device.clone(), passes.clone()));
        Self {
            device,
            passes,
            control,
        }
    }
}

pub fn router(server: &Server) -> Router {
    Router::new()
        .merge(http::router(server.device.clone()))
        .merge(crate::api::router(server.control.clone()))
        .fallback(http::unmatched)
        .layer(axum::middleware::from_fn(trace_request))
}

/// One line per request, at `debug` for the polls and renewals that would fill the log.
async fn trace_request(request: Request, next: axum::middleware::Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let peer = http::peer_of(&request);
    let headers: Vec<String> = request
        .headers()
        .iter()
        .filter(|(name, _)| {
            let n = name.as_str();
            n == "user-agent"
                || n == "callback"
                || n == "nt"
                || n == "timeout"
                || n == "sid"
                || n == "accept-encoding"
        })
        .map(|(name, value)| format!("{name}: {}", value.to_str().unwrap_or("?")))
        .collect();
    let asked = request.headers().clone();
    let response = next.run(request).await;
    let status = response.status().as_u16();
    let headers = headers.join("; ");
    let path = uri.path();
    if routine(&method, path, status, &asked) {
        tracing::debug!(%peer, %method, %path, status, %headers, "request");
    } else {
        tracing::info!(%peer, %method, %path, status, %headers, "request");
    }
    if uri.path().starts_with("/media/") || uri.path().starts_with("/art/") {
        http::trace_renderer(&peer, &method, uri.path(), status, &asked);
    }
    response
}

fn routine(method: &Method, path: &str, status: u16, headers: &HeaderMap) -> bool {
    if status >= 400 {
        return false;
    }
    (method == Method::GET && path.starts_with("/api/"))
        || (method.as_str() == "SUBSCRIBE" && headers.contains_key("sid"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with(name: &'static str, value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(name, value.parse().unwrap());
        headers
    }

    #[test]
    fn a_page_reading_the_interface_does_not_fill_the_log_it_is_read_beside() {
        let none = HeaderMap::new();
        assert!(routine(&Method::GET, "/api/status", 200, &none));
        assert!(routine(&Method::GET, "/api/config", 200, &none));
        assert!(
            !routine(&Method::POST, "/api/rescan", 202, &none),
            "asking for a pass is an event, not a poll"
        );
        assert!(
            !routine(&Method::GET, "/api/status", 500, &none),
            "a poll that failed is the one anybody wants to find"
        );
        assert!(
            !routine(&Method::GET, "/control/ContentDirectory", 200, &none),
            "what a control point asks for is the whole point of this log"
        );
    }

    #[test]
    fn a_subscriber_renewing_does_not_fill_the_log_and_a_new_one_is_written_down() {
        let subscribe = Method::from_bytes(b"SUBSCRIBE").expect("a method");
        let renewing = headers_with("sid", "uuid:one");
        assert!(routine(
            &subscribe,
            "/event/ContentDirectory",
            200,
            &renewing
        ));
        assert!(
            !routine(&subscribe, "/event/ContentDirectory", 412, &renewing),
            "a renewal that was refused is what somebody will look for"
        );
        let arriving = headers_with("callback", "<http://192.0.2.9:1/>");
        assert!(
            !routine(&subscribe, "/event/ContentDirectory", 200, &arriving),
            "a device arriving is an event"
        );
    }
}
