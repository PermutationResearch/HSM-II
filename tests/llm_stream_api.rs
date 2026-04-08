//! HTTP smoke tests for `POST /api/llm/chat/stream` (no live LLM required).

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hyper_stigmergy::api::{api_router, ApiState, SharedState};
use tower::ServiceExt;

#[tokio::test]
async fn llm_chat_stream_rejects_empty_messages() {
    let app = api_router(ApiState::new(SharedState::new()));
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/llm/chat/stream")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"messages":[]}"#))
                .unwrap(),
        )
        .await
        .expect("oneshot");
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}
