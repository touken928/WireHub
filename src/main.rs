mod api;
mod model;
mod flows;
mod network;
mod policy;
mod storage;
mod static_assets;
mod transport;
mod startup;
mod hub_key;

#[tokio::main]
async fn main() -> startup::Result<()> {
    if std::env::args().any(|a| a == "export-openapi") {
        print!("{}", api::openapi());
        return Ok(());
    }
    startup::run().await
}
