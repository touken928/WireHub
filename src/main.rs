mod api;
mod hub_key;
mod kernel;
mod model;
mod network;
mod startup;
mod static_assets;
mod storage;

#[tokio::main]
async fn main() -> startup::Result<()> {
    if std::env::args().any(|a| a == "export-openapi") {
        print!("{}", api::openapi());
        return Ok(());
    }
    startup::run().await
}
