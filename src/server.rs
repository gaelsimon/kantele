//! The three states one process runs, and the one router that serves them.

use std::sync::Arc;

use axum::Router;

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
        .layer(axum::middleware::from_fn(http::trace_request))
}
