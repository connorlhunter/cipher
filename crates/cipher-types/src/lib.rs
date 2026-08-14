use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ServiceStatus {
    pub status: &'static str,
}

impl ServiceStatus {
    pub const fn ready() -> Self {
        Self { status: "ok" }
    }
}
