//! Versioned HTTP response envelopes and error semantics for Cipher's API.

use axum::{
    Json,
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use uuid::Uuid;

/// The only HTTP API major version currently supported by Cipher.
pub const API_VERSION: &str = "v1";

/// The response header that identifies the negotiated API version.
pub const API_VERSION_HEADER: &str = "x-cipher-api-version";

/// Describes the successful JSON envelope used by versioned HTTP endpoints.
#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    /// Endpoint-specific response data.
    pub data: T,
    /// Cross-cutting response metadata.
    pub meta: ResponseMeta,
}

/// Metadata included with every successful or failed versioned API response.
#[derive(Debug, Serialize)]
pub struct ResponseMeta {
    /// The negotiated major API version.
    #[serde(rename = "apiVersion")]
    pub api_version: &'static str,
    /// An opaque identifier for correlating one request with support logs.
    #[serde(rename = "requestId")]
    pub request_id: String,
    /// Cursor metadata for a paginated collection response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<PageMeta>,
}

impl ResponseMeta {
    /// Builds response metadata for a non-paginated response.
    pub fn with_request_id(request_id: impl Into<String>) -> Self {
        Self {
            api_version: API_VERSION,
            request_id: request_id.into(),
            page: None,
        }
    }

    /// Builds response metadata for a collection response.
    pub fn with_page(request_id: impl Into<String>, next_cursor: Option<String>) -> Self {
        Self {
            api_version: API_VERSION,
            request_id: request_id.into(),
            page: Some(PageMeta { next_cursor }),
        }
    }
}

/// Opaque pagination information for a collection response.
#[derive(Debug, Serialize)]
pub struct PageMeta {
    /// Cursor to pass to the same collection endpoint for the next page.
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Describes a failed versioned API response.
#[derive(Debug, Serialize)]
pub struct ApiErrorResponse {
    /// Machine-readable failure details.
    pub error: ApiError,
    /// Cross-cutting response metadata.
    pub meta: ResponseMeta,
}

/// A stable error code with a safe client-facing message.
#[derive(Debug, Serialize)]
pub struct ApiError {
    /// Stable code clients use for branching behavior.
    pub code: ApiErrorCode,
    /// Safe, non-sensitive description of the failure.
    pub message: &'static str,
    /// Per-field validation failures when applicable.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<ValidationIssue>,
}

/// A client-correctable validation failure.
#[derive(Debug, Serialize)]
pub struct ValidationIssue {
    /// JSON field or query parameter that failed validation.
    pub field: &'static str,
    /// Stable, field-specific failure code.
    pub code: &'static str,
    /// Safe explanation suitable for a user-facing client.
    pub message: &'static str,
}

/// Stable error codes returned by the versioned HTTP API.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    /// The request envelope or parameters are malformed.
    InvalidRequest,
    /// The request has no valid access token.
    Unauthenticated,
    /// The authenticated principal lacks permission for the resource.
    Forbidden,
    /// The requested resource is not visible to the caller.
    NotFound,
    /// The requested API major version is unsupported.
    UnsupportedVersion,
    /// A state precondition no longer holds.
    Conflict,
    /// The request duplicates an existing operation.
    Duplicate,
    /// The request refers to state that has been superseded.
    Stale,
    /// The request or its replay key is outside the accepted time window.
    Expired,
    /// The request body exceeds the endpoint's maximum size.
    TooLarge,
    /// The caller must wait before sending another request.
    RateLimited,
    /// The service is temporarily unavailable.
    Unavailable,
    /// The server could not safely complete the request.
    Internal,
}

impl ApiErrorCode {
    /// Returns the HTTP status associated with this stable error code.
    pub const fn status(self) -> StatusCode {
        match self {
            Self::InvalidRequest => StatusCode::BAD_REQUEST,
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::UnsupportedVersion => StatusCode::NOT_ACCEPTABLE,
            Self::Conflict | Self::Duplicate | Self::Stale | Self::Expired => StatusCode::CONFLICT,
            Self::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Returns the stable safe message associated with this error code.
    pub const fn message(self) -> &'static str {
        match self {
            Self::InvalidRequest => "The request is malformed.",
            Self::Unauthenticated => "Authentication is required.",
            Self::Forbidden => "You do not have access to this resource.",
            Self::NotFound => "The requested resource was not found.",
            Self::UnsupportedVersion => "This API version is not supported.",
            Self::Conflict => "The resource state conflicts with this request.",
            Self::Duplicate => "The request duplicates an existing operation.",
            Self::Stale => "The request refers to state that has been superseded.",
            Self::Expired => "The request or replay key has expired.",
            Self::TooLarge => "The request body is too large.",
            Self::RateLimited => "Too many requests were sent. Try again later.",
            Self::Unavailable => "The service is temporarily unavailable.",
            Self::Internal => "The server could not complete the request.",
        }
    }
}

/// Generates an opaque request identifier for a server response.
pub fn new_request_id() -> String {
    format!("req_{}", Uuid::new_v4().simple())
}

/// Converts successful endpoint data into the versioned JSON envelope.
pub fn success<T: Serialize>(data: T, meta: ResponseMeta) -> Response {
    with_version_header((StatusCode::OK, Json(ApiResponse { data, meta })).into_response())
}

/// Converts a stable error code into the versioned JSON error envelope.
pub fn failure(code: ApiErrorCode, meta: ResponseMeta) -> Response {
    failure_with_details(code, meta, Vec::new())
}

/// Converts an error code and field details into the versioned JSON error envelope.
pub fn failure_with_details(
    code: ApiErrorCode,
    meta: ResponseMeta,
    details: Vec<ValidationIssue>,
) -> Response {
    failure_with_status(code.status(), code, meta, details)
}

/// Converts an error code and explicit HTTP status into a versioned error response.
///
/// `invalid_request` is used for several client-correctable transport failures,
/// including malformed JSON and an unsupported media type, whose HTTP statuses
/// differ while their stable wire code remains the same.
pub fn failure_with_status(
    status: StatusCode,
    code: ApiErrorCode,
    meta: ResponseMeta,
    details: Vec<ValidationIssue>,
) -> Response {
    let body = ApiErrorResponse {
        error: ApiError {
            code,
            message: code.message(),
            details,
        },
        meta,
    };
    with_version_header((status, Json(body)).into_response())
}

fn with_version_header(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(API_VERSION_HEADER, HeaderValue::from_static(API_VERSION));
    response
}
