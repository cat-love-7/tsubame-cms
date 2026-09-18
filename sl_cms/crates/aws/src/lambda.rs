//! Serving the shared router on AWS Lambda.
//!
//! `lambda_http` turns an invocation into an `http::Request` — whichever of API Gateway's REST
//! (payload 1.0) or HTTP API (payload 2.0), an ALB or a function URL sent it, base64 and all —
//! and hands it to a `tower` service. The CMS is already a `tower` service, because that is what
//! an axum router is, so what is left is the conversion in both directions and the one limit
//! that belongs to Lambda rather than to the CMS.

use axum::Router;
use axum::body::Body as AxumBody;
use lambda_http::http::{Request, Response, StatusCode, header};
use lambda_http::{Body, Error};
use tower::ServiceExt;

/// The largest request body this function will look at.
///
/// Lambda refuses an invocation over 6MB, and API Gateway base64-encodes a body it considers
/// binary, which is a third larger again — so a body of 6MB can arrive as an 8MB event that the
/// function never sees. A CMS request is JSON (images are uploaded straight to S3 with a
/// presigned URL and never pass through here), so four megabytes is the honest ceiling: below it
/// the worst case still fits, and above it the caller is told which limit they hit rather than
/// being left with a function that did not run.
pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

/// One invocation: the event's request through the router, the router's answer back.
///
/// Public so it can be driven by a synthetic event in a test — which is the only way to check
/// the event shapes without deploying.
pub async fn dispatch(router: Router, request: Request<Body>) -> Result<Response<Body>, Error> {
    let (parts, body) = request.into_parts();
    let body = into_bytes(body);

    if body.len() > MAX_BODY_BYTES {
        return Ok(text(
            StatusCode::PAYLOAD_TOO_LARGE,
            &format!(
                "the request body is {} bytes; this deployment accepts at most {MAX_BODY_BYTES} \
                 (image bytes go straight to S3, so nothing the CMS stores should come near it)",
                body.len()
            ),
        ));
    }

    let request = Request::from_parts(parts, AxumBody::from(body));
    let response = router
        .oneshot(request)
        .await
        .expect("the router answers every request");
    let (parts, body) = response.into_parts();
    let body = axum::body::to_bytes(body, usize::MAX)
        .await
        .map_err(|e| Error::from(format!("could not read the response body: {e}")))?;

    // `Binary` rather than `Text`: the runtime marks it base64 for API Gateway, which is what
    // carries any content type back intact.
    Ok(Response::from_parts(parts, Body::Binary(body.to_vec())))
}

/// The body of an event, whatever kind it is. A binary one has already been decoded from base64
/// by `lambda_http`, so these are the bytes the client sent.
fn into_bytes(body: Body) -> Vec<u8> {
    match body {
        Body::Empty => Vec::new(),
        Body::Text(text) => text.into_bytes(),
        Body::Binary(bytes) => bytes,
        // `Body` is `#[non_exhaustive]`: anything new is treated as binary rather than
        // dropped, which is the least surprising thing to do with a body.
        other => other.as_ref().to_vec(),
    }
}

/// A plain answer, in the shape the CMS's own errors use.
fn text(status: StatusCode, message: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::Text(message.to_string()))
        .expect("a response can be built")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::{get, post};
    use lambda_http::request::LambdaRequest;

    /// A router with no storage behind it: these tests are about the two event shapes, the
    /// base64 decoding and the size guard, not about the CMS.
    fn test_router() -> Router {
        Router::new().route("/", get(|| async { "ok" })).route(
            "/echo",
            post(|body: String| async move { format!("got:{body}") }),
        )
    }

    /// An API Gateway **HTTP API** event (payload 2.0), built the way the runtime builds one:
    /// through the same deserialization, so the shape is the real one.
    fn http_api_event(method: &str, path: &str, body: &str, base64: bool) -> Request<Body> {
        let event = serde_json::json!({
            "version": "2.0",
            "routeKey": format!("{method} {path}"),
            "rawPath": path,
            "rawQueryString": "",
            "headers": { "content-type": "application/json" },
            "body": body,
            "isBase64Encoded": base64,
            "requestContext": {
                "accountId": "1",
                "apiId": "test",
                "domainName": "test.example",
                "http": { "method": method, "path": path, "protocol": "HTTP/1.1", "sourceIp": "127.0.0.1" },
                "requestId": "test",
                "routeKey": format!("{method} {path}"),
                "stage": "$default",
                "time": "01/Jan/2026:00:00:00 +0000",
                "timeEpoch": 0
            }
        });
        let parsed: LambdaRequest = serde_json::from_value(event).expect("an HTTP API event");
        parsed.into()
    }

    /// An API Gateway **REST API** event (payload 1.0).
    fn rest_api_event(method: &str, path: &str) -> Request<Body> {
        let event = serde_json::json!({
            "resource": path,
            "path": path,
            "httpMethod": method,
            "headers": { "content-type": "application/json" },
            "queryStringParameters": null,
            "pathParameters": null,
            "stageVariables": null,
            "isBase64Encoded": false,
            "body": null,
            "requestContext": {
                "accountId": "1",
                "apiId": "test",
                "domainName": "test.example",
                "httpMethod": method,
                "path": path,
                "requestId": "test",
                "stage": "$default"
            }
        });
        let parsed: LambdaRequest = serde_json::from_value(event).expect("a REST API event");
        parsed.into()
    }

    #[tokio::test]
    async fn an_http_api_event_reaches_the_router() {
        let response = dispatch(test_router(), http_api_event("GET", "/", "", false))
            .await
            .expect("a response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), &Body::Binary(b"ok".to_vec()));
    }

    #[tokio::test]
    async fn a_rest_api_event_reaches_the_same_router() {
        // Both payload versions have to work: a deployment chooses one, and the CMS should not
        // have to care which.
        let response = dispatch(test_router(), rest_api_event("GET", "/"))
            .await
            .expect("a response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn a_base64_body_arrives_as_the_bytes_it_stands_for() {
        // "hello" in base64 — what API Gateway sends for a body it treats as binary.
        let response = dispatch(
            test_router(),
            http_api_event("POST", "/echo", "aGVsbG8=", true),
        )
        .await
        .expect("a response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), &Body::Binary(b"got:hello".to_vec()));
    }

    #[tokio::test]
    async fn a_body_over_the_lambda_limit_is_refused_with_a_reason() {
        let oversized = "a".repeat(MAX_BODY_BYTES + 1);
        let response = dispatch(
            test_router(),
            http_api_event("POST", "/echo", &oversized, false),
        )
        .await
        .expect("a response");

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let Body::Text(message) = response.body() else {
            panic!("expected a text answer");
        };
        assert!(
            message.contains(&MAX_BODY_BYTES.to_string()),
            "the answer should name the limit: {message}"
        );
    }
}
