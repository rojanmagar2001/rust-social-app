#![allow(unused)]
use std::sync::Arc;

use anyhow::Context;
use axum::Router;
use sqlx::PgPool;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::{add_extension::AddExtensionLayer, trace::TraceLayer};

use crate::config::Config;

#[derive(Clone)]
struct ApiContext {
    config: Arc<Config>,
    db: PgPool,
}

pub async fn serve(config: Config, db: PgPool) -> anyhow::Result<()> {
    let app = api_router().layer(
        ServiceBuilder::new()
            .layer(AddExtensionLayer::new(ApiContext {
                config: Arc::new(config),
                db,
            }))
            .layer(TraceLayer::new_for_http()),
    );
    let tcp_listener = TcpListener::bind("127.0.0.1:3000")
        .await
        .context("error binding TCP listener")?;

    println!("Listening on {}", tcp_listener.local_addr()?);

    axum::serve(tcp_listener, app.into_make_service())
        .await
        .context("error running HTTP server")?;

    Ok(())
}

fn api_router() -> Router {
    Router::new()
}
