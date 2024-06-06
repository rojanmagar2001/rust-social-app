use crate::http::extractor::{AuthUser, MaybeAuthUser};
use crate::http::{Error, Result};
use axum::extract::Path;
use axum::routing::get;
use axum::{Extension, Json, Router};

use super::ApiContext;

pub fn router() -> Router {
    Router::new()
        .route("/api/profiles", get(get_users_profile))
        .route("/api/profiles/:id", get(get_user_profile))
}

#[derive(Debug, serde::Serialize)]
struct ProfileBody {
    profile: Profile,
}

#[derive(Debug, serde::Serialize)]
struct ProfilesBody {
    profiles: Vec<Profile>,
}

#[derive(Debug, serde::Serialize)]
pub struct Profile {
    pub username: String,
    pub bio: Option<String>,
    pub image: Option<String>,
    pub following: bool,
}

async fn get_users_profile(
    maybe_auth_user: MaybeAuthUser,
    ctx: Extension<ApiContext>,
) -> Result<Json<ProfilesBody>> {
    let profiles = sqlx::query_as!(
        Profile,
        r#"
            select
                username,
                bio,
                image,
                exists(
                    select 1 from follow
                    where
                        followed_user_id = "user".user_id and
                        following_user_id = $1
                ) "following!"
            from "user" where "user".user_id != $1
        "#,
        maybe_auth_user.user_id(),
    )
    .fetch_all(&ctx.db)
    .await
    .map_err(|e| {
        log::error!("Database error: {:?}", e);
        Error::Sqlx(e)
    })?;

    Ok(Json(ProfilesBody { profiles }))
}

async fn get_user_profile(
    auth_user: AuthUser,
    ctx: Extension<ApiContext>,
    Path(username): Path<String>,
) -> Result<Json<ProfileBody>> {
    let profile = sqlx::query_as!(
        Profile,
        r#"
            select
                username,
                bio,
                image,
                exists(
                    select 1 from follow
                    where
                        followed_user_id = "user".user_id and
                        following_user_id = $2
                ) "following!"
            from "user"
            where username = $1
        "#,
        username,
        auth_user.user_id,
    )
    .fetch_optional(&ctx.db)
    .await?
    .ok_or(Error::NotFound)?;

    Ok(Json(ProfileBody { profile }))
}
