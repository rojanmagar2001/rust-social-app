use anyhow::Context;
use clap::Parser;
use rust_social_app::{config::Config, http};
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // This returns an error if the .env file is not found
    dotenv::dotenv().ok();

    // Initialize the logger.
    env_logger::init();

    // Parse our configuration from the environment.
    // This will exit with a help message if something is wrong.
    let config = Config::parse();

    // Initialize the database connection pool.
    let db = PgPoolOptions::new()
        .max_connections(50)
        .connect(&config.database_url)
        .await
        .context("error connecting to the database")?;

    http::serve(config, db).await?;

    Ok(())
}
