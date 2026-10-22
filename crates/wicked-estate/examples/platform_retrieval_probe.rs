//! Consumer-only probe; compiled in a disposable exact-revision archive.
use serde_json::json;
use wicked_estate_core::RetrievalTool;
use wicked_estate_retrieve::{ContextPack, FetchContent, SearchEntity};
use wicked_estate_store::PostgresStore;
fn main() {
    if let Err(e) = run() {
        eprintln!("ESTATE_PROBE_REFUSED: {e}");
        std::process::exit(2);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::env::args().nth(1).ok_or("missing mode")?;
    let url = std::env::var("ESTATE_PROBE_DATABASE")?;
    let mut store = if mode == "index" {
        PostgresStore::open(&url)?
    } else {
        PostgresStore::connect_existing(&url)?
    };
    if mode == "index" {
        let root = std::env::var("ESTATE_PROBE_MATERIAL")?;
        wicked_estate::index_path_as(
            &mut store,
            std::path::Path::new(&root),
            Some("cu13_snapshot"),
        )?;
    } else if mode != "read" {
        return Err("unsupported mode".into());
    }
    let search = SearchEntity.invoke(&store, &json!({"name":"invoiceTotalCents","limit":5}))?;
    let mut excerpts = Vec::new();
    for m in search.content["matches"]
        .as_array()
        .ok_or("missing matches")?
    {
        if let Some(symbol) = m["symbol"].as_str() {
            excerpts.push(
                FetchContent
                    .invoke(&store, &json!({"symbol":symbol}))?
                    .content,
            );
        }
    }
    let missing = FetchContent.invoke(&store, &json!({"symbol":"forged/foreign/symbol"}))?;
    let context = ContextPack.invoke(
        &store,
        &json!({"name":"invoiceTotalCents","token_budget":128}),
    )?;
    println!(
        "{}",
        json!({"search":search.content,"diagnostics":search.diagnostics,"excerpts":excerpts,"missing":missing.content,"context":context.content,"semantic":"not_qualified"})
    );
    Ok(())
}
