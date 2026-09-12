use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn openai_shape_compat() {
    let app = nine_gateway::router();
    let payload = std::fs::read_to_string(format!(
        "{}/../../fixtures/openai/chat.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let resp = app
        .oneshot(
            Request::post("/v1/chat/completions")
                .header("content-type", "application/json")
                .header("authorization", "Bearer k")
                .body(Body::from(payload))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let b = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["object"], "chat.completion");
    assert!(v.get("usage").is_some());
}
