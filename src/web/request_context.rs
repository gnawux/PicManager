use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};

use crate::application::{Application, CallerKind, RequestContext};

pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

pub async fn attach(
    State(application): State<Application>,
    mut request: Request,
    next: Next,
) -> Response {
    let supplied_request_id = request
        .headers()
        .get(&REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| valid_request_id(value));
    let mut context = application.request_context(CallerKind::LocalWeb);
    if let Some(request_id) = supplied_request_id {
        context = context.with_request_id(request_id);
    }
    let response_request_id = context.request_id.clone();
    request.extensions_mut().insert(context);

    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&response_request_id) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
    response
}

fn valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

pub fn internal_context(application: &Application, job_id: i64) -> RequestContext {
    application
        .request_context(CallerKind::InternalWorker)
        .with_request_id(Arc::<str>::from(format!("job-{job_id}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_safe_correlation_ids_only() {
        assert!(valid_request_id("mac-app:sync_42.1"));
        assert!(!valid_request_id(""));
        assert!(!valid_request_id("contains spaces"));
        assert!(!valid_request_id("../header"));
        assert!(!valid_request_id(&"x".repeat(129)));
    }
}
