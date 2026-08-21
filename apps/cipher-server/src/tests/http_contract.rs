use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use serde::Serialize;
use tower::ServiceExt;

use super::{app, http_contract, requested_api_version};

const REQUEST_ID: &str = "req_01f4c7c7dc9d4cf9a0d4d66a7fa8b24b";

#[derive(Serialize)]
struct Status<'a> {
    status: &'a str,
}

#[tokio::test]
async fn success_envelope_matches_the_golden_example() {
    let response = http_contract::success(
        Status { status: "ok" },
        http_contract::ResponseMeta::with_request_id(REQUEST_ID),
    );

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(
        response.headers()[http_contract::API_VERSION_HEADER],
        http_contract::API_VERSION
    );
    assert_json_fixture(
        &response_body(response).await,
        include_str!("fixtures/http-v1/success.json"),
    );
}

#[tokio::test]
async fn denial_conflict_and_expiry_match_the_golden_examples() {
    let cases = [
        (
            http_contract::ApiErrorCode::Forbidden,
            include_str!("fixtures/http-v1/denial.json"),
        ),
        (
            http_contract::ApiErrorCode::Conflict,
            include_str!("fixtures/http-v1/idempotency-conflict.json"),
        ),
        (
            http_contract::ApiErrorCode::Expired,
            include_str!("fixtures/http-v1/idempotency-expired.json"),
        ),
    ];

    for (code, fixture) in cases {
        let response = http_contract::failure(
            code,
            http_contract::ResponseMeta::with_request_id(REQUEST_ID),
        );

        assert_eq!(response.status(), code.status());
        assert_json_fixture(&response_body(response).await, fixture);
    }
}

#[tokio::test]
async fn validation_failure_matches_the_golden_example() {
    let response = http_contract::failure_with_details(
        http_contract::ApiErrorCode::InvalidRequest,
        http_contract::ResponseMeta::with_request_id(REQUEST_ID),
        vec![http_contract::ValidationIssue {
            field: "limit",
            code: "out_of_range",
            message: "limit must be between 1 and 100.",
        }],
    );

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_json_fixture(
        &response_body(response).await,
        include_str!("fixtures/http-v1/validation.json"),
    );
}

#[tokio::test]
async fn unsupported_version_matches_the_golden_example() {
    let response = http_contract::failure(
        http_contract::ApiErrorCode::UnsupportedVersion,
        http_contract::ResponseMeta::with_request_id(REQUEST_ID),
    );

    assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
    assert_json_fixture(
        &response_body(response).await,
        include_str!("fixtures/http-v1/unsupported-version.json"),
    );
}

#[tokio::test]
async fn collection_metadata_keeps_the_next_cursor_opaque() {
    let response = http_contract::success(
        Vec::<&str>::new(),
        http_contract::ResponseMeta::with_page(REQUEST_ID, Some("next-page".into())),
    );
    let body = response_body(response).await;

    assert!(body.contains("\"nextCursor\":\"next-page\""));

    let final_page = http_contract::success(
        Vec::<&str>::new(),
        http_contract::ResponseMeta::with_page(REQUEST_ID, None),
    );
    assert!(response_body(final_page).await.contains("\"page\":{}"));
}

#[test]
fn error_codes_have_stable_statuses_and_messages() {
    let cases = [
        (
            http_contract::ApiErrorCode::InvalidRequest,
            StatusCode::BAD_REQUEST,
            "The request is malformed.",
        ),
        (
            http_contract::ApiErrorCode::Unauthenticated,
            StatusCode::UNAUTHORIZED,
            "Authentication is required.",
        ),
        (
            http_contract::ApiErrorCode::Forbidden,
            StatusCode::FORBIDDEN,
            "You do not have access to this resource.",
        ),
        (
            http_contract::ApiErrorCode::NotFound,
            StatusCode::NOT_FOUND,
            "The requested resource was not found.",
        ),
        (
            http_contract::ApiErrorCode::UnsupportedVersion,
            StatusCode::NOT_ACCEPTABLE,
            "This API version is not supported.",
        ),
        (
            http_contract::ApiErrorCode::Conflict,
            StatusCode::CONFLICT,
            "The resource state conflicts with this request.",
        ),
        (
            http_contract::ApiErrorCode::Duplicate,
            StatusCode::CONFLICT,
            "The request duplicates an existing operation.",
        ),
        (
            http_contract::ApiErrorCode::Stale,
            StatusCode::CONFLICT,
            "The request refers to state that has been superseded.",
        ),
        (
            http_contract::ApiErrorCode::Expired,
            StatusCode::CONFLICT,
            "The request or replay key has expired.",
        ),
        (
            http_contract::ApiErrorCode::TooLarge,
            StatusCode::PAYLOAD_TOO_LARGE,
            "The request body is too large.",
        ),
        (
            http_contract::ApiErrorCode::Unavailable,
            StatusCode::SERVICE_UNAVAILABLE,
            "The service is temporarily unavailable.",
        ),
        (
            http_contract::ApiErrorCode::RateLimited,
            StatusCode::TOO_MANY_REQUESTS,
            "Too many requests were sent. Try again later.",
        ),
        (
            http_contract::ApiErrorCode::Internal,
            StatusCode::INTERNAL_SERVER_ERROR,
            "The server could not complete the request.",
        ),
    ];

    for (code, status, message) in cases {
        assert_eq!(code.status(), status);
        assert_eq!(code.message(), message);
    }
}

#[tokio::test]
async fn content_type_errors_keep_the_shared_invalid_request_code() {
    let response = http_contract::failure_with_status(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        http_contract::ApiErrorCode::InvalidRequest,
        http_contract::ResponseMeta::with_request_id(REQUEST_ID),
        Vec::new(),
    );

    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(
        response_body(response)
            .await
            .contains("\"code\":\"invalid_request\"")
    );
}

#[test]
fn request_ids_are_opaque_uuid_values() {
    let first = http_contract::new_request_id();
    let second = http_contract::new_request_id();

    assert_ne!(first, second);
    for request_id in [first, second] {
        let uuid = request_id.strip_prefix("req_").unwrap();
        assert_eq!(uuid.len(), 32);
        assert!(uuid.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}

#[tokio::test]
async fn api_descriptor_is_served_in_the_versioned_envelope() {
    let response = app()
        .oneshot(Request::builder().uri("/v1").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[http_contract::API_VERSION_HEADER],
        http_contract::API_VERSION
    );
    let body = response_body(response).await;
    assert!(body.contains("\"apiVersion\":\"v1\""));
    assert!(body.contains("\"mediaType\":\"application/json\""));
    assert!(body.contains("\"requestId\":\"req_"));
}

#[tokio::test]
async fn versioned_fallbacks_return_contract_errors() {
    let unsupported = app()
        .oneshot(
            Request::builder()
                .uri("/v2/conversations")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unsupported.status(), StatusCode::NOT_ACCEPTABLE);
    assert_eq!(
        unsupported.headers()[http_contract::API_VERSION_HEADER],
        http_contract::API_VERSION
    );
    assert!(
        response_body(unsupported)
            .await
            .contains("\"code\":\"unsupported_version\"")
    );

    let unavailable = app()
        .oneshot(
            Request::builder()
                .uri("/v1/not-implemented")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unavailable.status(), StatusCode::NOT_FOUND);
    assert!(
        response_body(unavailable)
            .await
            .contains("\"code\":\"not_found\"")
    );

    let non_api = app()
        .oneshot(
            Request::builder()
                .uri("/not-an-api-route")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(non_api.status(), StatusCode::NOT_FOUND);
    assert!(
        non_api
            .headers()
            .get(http_contract::API_VERSION_HEADER)
            .is_none()
    );
}

#[test]
fn requested_api_version_only_accepts_a_major_version_path_segment() {
    assert_eq!(requested_api_version("/v1/messages"), Some("v1"));
    assert_eq!(requested_api_version("/v2"), Some("v2"));
    assert_eq!(requested_api_version("/v"), None);
    assert_eq!(requested_api_version("/v1alpha/messages"), None);
    assert_eq!(requested_api_version("/version/messages"), None);
    assert_eq!(requested_api_version("v1/messages"), None);
}

async fn response_body(response: Response) -> String {
    String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap()
}

fn assert_json_fixture(actual: &str, fixture: &str) {
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(actual).unwrap(),
        serde_json::from_str::<serde_json::Value>(fixture).unwrap()
    );
}
