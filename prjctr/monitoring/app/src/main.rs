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

use prometheus_exporter::{
    self,
    prometheus::Counter,
    prometheus::register_counter,
};

#[derive(Parser)]
struct Cli {
    #[arg(default_value = "0.0.0.0:5000", env = "LISTEN_ADDR")]
    server_addr: String,
    #[arg(default_value = "mongodb://admin:admin@localhost:27017", env = "MONGO_ADDR")]
    mongo_addr: String,
    #[arg(default_value = "http://localhost:9200", env = "ES_ADDR")]
    es_addr: String,
    #[arg(default_value = "0.0.0.0:9090", env = "PROM_LISTEN_ADDR")]
    prom_addr: String,
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
    
    let binding = args.prom_addr.parse().unwrap();
    prometheus_exporter::start(binding).unwrap();
    
    let es_requests = register_counter!("app_requests_es", "requests").unwrap();
    let mongo_requests = register_counter!("app_requests_mongo", "requests").unwrap();
    let es_failures = register_counter!("app_failures_es", "requests").unwrap();
    let mongo_failures = register_counter!("app_failures_mongo", "requests").unwrap();
    let metrics = Metrics {
        es_requests,
        mongo_requests,
        es_failures,
        mongo_failures,
    };

    let transport = Transport::single_node(&args.es_addr)?;
    let es_client = Elasticsearch::new(transport);

    let state = Arc::new((mongo_db, es_client, metrics));
    let app = Router::new()
        .route("/", post(handler))
        .with_state(state);

    axum::Server::bind(&args.server_addr.parse().unwrap())
        .serve(app.into_make_service())
        .await?;
    Ok(())
}

struct Metrics {
    es_requests: Counter,
    mongo_requests: Counter,
    es_failures: Counter,
    mongo_failures: Counter,
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
    State(state): State<Arc<(Database, Elasticsearch, Metrics)>>,
    Json(payload): Json<Request>,
) -> Result<Json<Response>, StatusCode> {
    Ok(Json(handler_internal(&state, payload).await?))
}

async fn handler_internal(
    (mongo_db, es_client, Metrics {
        es_requests,
        mongo_requests,
        es_failures,
        mongo_failures,
    }): &(Database, Elasticsearch, Metrics),
    Request { key, value, db }: Request,
) -> Result<Response, StatusCode> {
    tracing::debug!("trying to serve {db}");
    match db.as_ref() {
        "mongo" => {
            mongo_requests.inc();
            let collection = mongo_db.collection::<Document>("kv");
            collection
                .insert_one(doc! { "key" : key.clone(), "value": value}, None)
                .await
                .map_err(|e| {
                    tracing::error!("failed to write to storage: {:?}", e);
                    mongo_failures.inc();
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            let filter = doc! { "key": key };
            let cursor = collection.find(filter, None).await.map_err(|e| {
                tracing::error!("failed to find in storage: {:?}", e);
                mongo_failures.inc();
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            let values: Vec<_> = cursor
                .map(|res| res.map(|item| item.get_str("value").map(|str| str.to_owned())))
                .filter_map(|item| async move { item.ok() })
                .try_collect()
                .await
                .map_err(|e| {
                    tracing::error!("failed to read from storage: {:?}", e);
                    mongo_failures.inc();
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            Ok(Response { values })
        }
        "elastic" => {
            es_requests.inc();
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
                    es_failures.inc();
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
                    tracing::error!("failed to search in storage: {:?}", e);
                    es_failures.inc();
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

            let response_body = response.json::<Value>().await.map_err(|e| {
                tracing::error!("failed to read from storage: {:?}", e);
                es_failures.inc();
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
