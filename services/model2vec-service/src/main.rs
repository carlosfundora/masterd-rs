use std::{env, net::SocketAddr, sync::Arc};

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use model2vec_rs::model::StaticModel;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct AppState {
    model: Arc<StaticModel>,
    model_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmbedRequest {
    #[serde(default)]
    input: Option<Vec<String>>,
    #[serde(default)]
    texts: Option<Vec<String>>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    status: &'static str,
    model: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EmbeddingDatum {
    embedding: Vec<f32>,
    index: usize,
    object: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
    dim: usize,
    data: Vec<EmbeddingDatum>,
    model: String,
    object: &'static str,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let model_name = env::var("MODEL2VEC_MODEL").unwrap_or_else(|_| "minishlab/potion-base-8M".to_string());
    let port = env::var("MODEL2VEC_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(11448);
    let addr: SocketAddr = SocketAddr::from(([127, 0, 0, 1], port));

    println!("loading model2vec model: {model_name}");
    let model = StaticModel::from_pretrained(&model_name, None, None, None)?;

    let app = Router::new()
        .route("/health", get(health))
        .route("/embed", post(embed))
        .route("/v1/embeddings", post(embed))
        .with_state(AppState {
            model: Arc::new(model),
            model_name,
        });

    println!("model2vec-service listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        model: state.model_name.clone(),
    })
}

async fn embed(
    State(state): State<AppState>,
    Json(req): Json<EmbedRequest>,
) -> Result<Json<EmbedResponse>, (StatusCode, Json<ErrorResponse>)> {
    let _requested_model = req.model;
    let texts = req
        .input
        .or(req.texts)
        .ok_or_else(|| bad_request("either 'input' or 'texts' is required"))?;

    if texts.is_empty() {
        return Ok(Json(EmbedResponse {
            embeddings: Vec::new(),
            dim: 0,
            data: Vec::new(),
            model: state.model_name.clone(),
            object: "list",
        }));
    }

    let embeddings = state.model.encode(&texts);
    let dim = embeddings.first().map(|embedding| embedding.len()).unwrap_or(0);
    let data = embeddings
        .iter()
        .enumerate()
        .map(|(index, embedding)| EmbeddingDatum {
            embedding: embedding.clone(),
            index,
            object: "embedding",
        })
        .collect::<Vec<_>>();

    Ok(Json(EmbedResponse {
        embeddings,
        dim,
        data,
        model: state.model_name.clone(),
        object: "list",
    }))
}

fn bad_request(message: impl Into<String>) -> (StatusCode, Json<ErrorResponse>) {
    (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: message.into() }))
}
