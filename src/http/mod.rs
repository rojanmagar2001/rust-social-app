#![allow(unused)]
use std::sync::Arc;

use anyhow::Context;
use axum::Router;
use sqlx::PgPool;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::{add_extension::AddExtensionLayer, trace::TraceLayer};

use crate::config::Config;

// Utility modules.

/// Defines a common error type to use for all request handlers, compliant with the Realworld spec.
mod error;

// Modules introducing API routes. The names match the routes listed in the Realworld spec,
// although the `articles` module also includes the `GET /api/tags` route because it touches
// the `article` table.
//
// This is not the order they were written in; `rustfmt` auto-sorts them.
// However, you should follow the order they were written in because some of the comments
// are more stream-of-consciousness and assume you read them in a particular order.
//
// See `api_router()` below for the recommended order.
mod extractor;
mod profiles;
mod users;
mod validator;

pub use error::{Error, ResultExt};

pub type Result<T, E = Error> = std::result::Result<T, E>;

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
    let tcp_listener = TcpListener::bind("127.0.0.1:8080")
        .await
        .context("error binding TCP listener")?;

    println!("Listening on {}", tcp_listener.local_addr()?);

    axum::serve(tcp_listener, app.into_make_service())
        .await
        .context("error running HTTP server")?;

    Ok(())
}

fn api_router() -> Router {
    users::router().merge(profiles::router())
}
