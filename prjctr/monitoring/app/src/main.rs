use std::{str::FromStr, sync::Arc};

use axum::{
    extract::{Json, State},
    http::StatusCode,
    routing::post,
    Router,
};

use elasticsearch::{http::transport::Transport, Elasticsearch, CreateParts, SearchParts};
use futures::stream::{StreamExt, TryStreamExt};
use mongodb::{
    bson::{doc, Document},
    options::ClientOptions,
    Client, Database,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing_subscriber::{filter::Targets, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
struct Cli {
    #[arg(default_value = "0.0.0.0:5000", env = "LISTEN_ADDR")]
    server_addr: String,
    #[arg(default_value = "mongodb://admin:admin@localhost:27017", env = "MONGO_ADDR")]
    mongo_addr: String,
    #[arg(default_value = "http://localhost:9200", env = "ES_ADDR")]
    es_addr: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter_layer =
        Targets::from_str(std::env::var("RUST_LOG").as_deref().unwrap_or("info")).unwrap();
    let format_layer = tracing_subscriber::fmt::layer();
    tracing_subscriber::registry()
        .with(filter_layer)
        .with(format_layer)
        .init();

    let args = Cli::parse();
    let client_options = ClientOptions::parse(&args.mongo_addr).await?;
    let mongo_client = Client::with_options(client_options)?;
    let mongo_db = mongo_client.database("mydb");

    let transport = Transport::single_node(&args.es_addr)?;
    let es_client = Elasticsearch::new(transport);

    let state = Arc::new((mongo_db, es_client));
    let app = Router::new()
        .route("/", post(handler))
        .with_state(state);

    axum::Server::bind(&args.server_addr.parse().unwrap())
        .serve(app.into_make_service())
        .await?;
    Ok(())
}

#[derive(Deserialize)]
struct Request {
    key: String,
    value: String,
    db: String,
}

#[derive(Serialize)]
struct Response {
    values: Vec<String>,
}

async fn handler(
    State(state): State<Arc<(Database, Elasticsearch)>>,
    Json(payload): Json<Request>,
) -> Result<Json<Response>, StatusCode> {
    Ok(Json(handler_internal(&state, payload).await?))
}

async fn handler_internal(
    (mongo_db, es_client): &(Database, Elasticsearch),
    Request { key, value, db }: Request,
) -> Result<Response, StatusCode> {
    match db.as_ref() {
        "mongo" => {
            let collection = mongo_db.collection::<Document>("kv");
            collection
                .insert_one(doc! { "key" : key.clone(), "value": value}, None)
                .await
                .map_err(|e| {
                    tracing::error!("failed to write to storage: {:?}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            let filter = doc! { "key": key };
            let cursor = collection.find(filter, None).await.map_err(|e| {
                tracing::error!("failed to find in storage: {:?}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            let values: Vec<_> = cursor
                .map(|res| res.map(|item| item.get_str("value").map(|str| str.to_owned())))
                .filter_map(|item| async move { item.ok() })
                .try_collect()
                .await
                .map_err(|e| {
                    tracing::error!("failed to read from storage: {:?}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            Ok(Response { values })
        }
        "elastic" => {
            es_client
                .create(CreateParts::IndexId("kv", &key))
                .body(json!({
                    "key": key.clone(),
                    "value": value,
                }))
                .send()
                .await
                .map_err(|e| {
                    tracing::error!("failed to write to storage: {:?}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

            let response = es_client
                .search(SearchParts::Index(&["kv"]))
                .from(0)
                .size(100)
                .body(json!({
                    "query": {
                        "match": {
                            "key": key
                        }
                    }
                }))
                .send()
                .await
                .map_err(|e| {
                    tracing::error!("failed to write to storage: {:?}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

            let response_body = response.json::<Value>().await.map_err(|e| {
                tracing::error!("failed to write to storage: {:?}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            let values = response_body["hits"]["hits"]
                .as_array()
                .unwrap()
                .into_iter()
                .filter_map(|hit| {
                    hit["_source"]
                        .get("value")
                        .map(|value| value.as_str().map(|str| str.to_owned()))
                        .flatten()
                })
                .collect();
            Ok(Response { values })
        }
        _ => Err(StatusCode::BAD_REQUEST),
    }
}
