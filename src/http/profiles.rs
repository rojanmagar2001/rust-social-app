use crate::http::extractor::{AuthUser, MaybeAuthUser};
use crate::http::{Error, Result, ResultExt};
use axum::extract::Path;
use axum::routing::{get, post};
use axum::{Extension, Json, Router};

use super::ApiContext;

pub fn router() -> Router {
    Router::new()
        .route("/api/profiles", get(get_users_profile))
        .route("/api/profiles/:id", get(get_user_profile))
        .route(
            "/api/profiles/:username/follow",
            post(follow_user).delete(unfollow_user),
        )
}

#[derive(Debug, serde::Serialize)]
struct ProfileBody {
    success: bool,
    data: Profile,
}

#[derive(Debug, serde::Serialize)]
struct ProfilesBody {
    success: bool,
    data: Vec<Profile>,
}

#[derive(Debug, serde::Serialize)]
pub struct Profile {
    pub username: String,
    pub bio: String,
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

    Ok(Json(ProfilesBody {
        success: true,
        data: profiles,
    }))
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

    Ok(Json(ProfileBody {
        success: true,
        data: profile,
    }))
}

async fn follow_user(
    auth_user: AuthUser,
    ctx: Extension<ApiContext>,
    Path(username): Path<String>,
) -> Result<Json<ProfileBody>> {
    let mut tx = ctx.db.begin().await?;

    let user = sqlx::query!(
        r#"
            select user_id, username, bio, image from "user" where username = $1
        "#,
        username,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;

    sqlx::query!(
        r#"
            insert into follow (following_user_id, followed_user_id)
            values ($1, $2) 
            on conflict do nothing
        "#,
        auth_user.user_id,
        user.user_id,
    )
    .execute(&mut *tx)
    .await
    .on_constraint("user_cannot_follow_self", |_| Error::Forbidden)?;

    tx.commit().await?;

    Ok(Json(ProfileBody {
        success: true,
        data: Profile {
            username: user.username,
            bio: user.bio,
            image: user.image,
            following: true,
        },
    }))
}

async fn unfollow_user(
    auth_user: AuthUser,
    ctx: Extension<ApiContext>,
    Path(username): Path<String>,
) -> Result<Json<ProfileBody>> {
    let mut tx = ctx.db.begin().await?;

    let user = sqlx::query!(
        r#"
            select user_id, username, bio, image from "user" where username = $1
        "#,
        username,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;

    sqlx::query!(
        r#"
            delete from follow
            where following_user_id = $1 and followed_user_id = $2
        "#,
        auth_user.user_id,
        user.user_id,
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(ProfileBody {
        success: true,
        data: Profile {
            username: user.username,
            bio: user.bio,
            image: user.image,
            following: false,
        },
    }))
}
