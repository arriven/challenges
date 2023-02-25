use chrono::{Datelike, Utc};
use clap::Parser;
use serde_json::{json, Value};

#[derive(Parser)]
struct Cli {
    #[arg(long, env = "GA4_MEASUREMENT_ID")]
    measurement_id: String,
    #[arg(long, env = "GA4_API_SECRET")]
    api_secret: String,
    #[arg(long, env = "GA4_CLIENT_ID")]
    client_id: String,
    #[arg(long, env = "DEBUG", default_value_t = false)]
    debug: bool,
    #[arg(long, default_value = "UAH")]
    base_currency: String,
    #[arg(long, default_value = "USD")]
    currency: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    let resp = exchange_rate(&args).await?;
    send_analytics(resp, &args).await
}

async fn exchange_rate(
    Cli {
        base_currency,
        currency,
        ..
    }: &Cli,
) -> anyhow::Result<Value> {
    let now = Utc::now();
    let resp: Value = reqwest::get(format!(
        "https://api.privatbank.ua/p24api/exchange_rates?json&date={day:02}.{month:02}.{year:04}",
        day = now.day(),
        month = now.month(),
        year = now.year(),
    ))
    .await?
    .json()
    .await?;
    let exchange_pair = resp
        .get("exchangeRate")
        .ok_or(anyhow::anyhow!("missing exchangeRate in response"))?
        .as_array()
        .ok_or(anyhow::anyhow!("wrong type for exchangeRate in response"))?
        .into_iter()
        .find(|item| {
            item.get("baseCurrency").unwrap().as_str().unwrap() == base_currency
                && item.get("currency").unwrap().as_str().unwrap() == currency
        })
        .ok_or(anyhow::anyhow!(
            "missing target pair in response: {base_currency}/{currency}"
        ))?;
    Ok(exchange_pair.clone())
}

async fn send_analytics(
    value: Value,
    Cli {
        measurement_id,
        api_secret,
        client_id,
        debug,
        ..
    }: &Cli,
) -> anyhow::Result<()> {
    let debug_api = if *debug { "/debug" } else { "" };
    let client = reqwest::Client::new();
    let resp = client.post(format!("https://www.google-analytics.com{debug_api}/mp/collect?measurement_id={measurement_id}&api_secret={api_secret}"))
        .json(&json!({
            "client_id": client_id,
            "events": [{
                "name": "exchange_rate",
                "params": value
            }]
        }))
        .send()
        .await?
        .text()
        .await?;
    if !resp.is_empty() {
        println!("{resp}");
    }
    Ok(())
}
