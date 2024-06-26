use async_trait::async_trait;
use axum::{
    body::Body,
    extract::{Extension, FromRequest, FromRequestParts, Request},
    http::{header::AUTHORIZATION, request::Parts, HeaderValue},
    RequestPartsExt,
};
use axum_valid::query;
use hmac::{Hmac, Mac};
use jwt::{SignWithKey, VerifyWithKey};
use serde::{Deserialize, Serialize};
use sha2::Sha384;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::http::{error::Error, ApiContext};

const DEFAULT_SESSION_LENGTH: Duration = Duration::weeks(2);

const SCHEME_PREFIX: &str = "Bearer ";

/// Add this as a parameter to a handler function to require the user to be logged in.
///
/// Parses a JWT from the `Authorization: Token <token>` header.
pub struct AuthUser {
    pub user_id: Uuid,
}

/// Add this as a parameter to a handler function to optionally check if the user is logged in.
///
/// If the `Authorization` header is absent then this will be `Self(None)`, otherwise it will
/// validate the token.
///
/// This is in contrast to directly using `Option<AuthUser>`, which will be `None` if there
/// is *any* error in deserializing, which isn't exactly what we want.
pub struct MaybeAuthUser(Option<AuthUser>);

#[derive(Serialize, Deserialize)]
struct AuthUserClaims {
    user_id: Uuid,
    // Standard JWT `exp` claim.
    exp: i64,
}

impl AuthUser {
    pub(in crate::http) fn to_jwt(&self, ctx: &ApiContext) -> String {
        let hmac = Hmac::<Sha384>::new_from_slice(ctx.config.hmac_key.as_bytes())
            .expect("HMAC-SHA-384 can accept any key length");

        AuthUserClaims {
            user_id: self.user_id,
            exp: (OffsetDateTime::now_utc() + DEFAULT_SESSION_LENGTH).unix_timestamp(),
        }
        .sign_with_key(&hmac)
        .expect("HMAC signing should be infallible")
    }

    // Attempt to parse `Self` from an `Authorization` header.
    async fn from_authorization(
        ctx: &ApiContext,
        auth_header: &HeaderValue,
    ) -> Result<Self, Error> {
        let auth_header = auth_header.to_str().map_err(|_| {
            log::debug!("Authorization header was not valid UTF-8");
            Error::Unathorized
        })?;

        if !auth_header.starts_with(SCHEME_PREFIX) {
            log::debug!(
                "Authorization header did not start with '{}'",
                SCHEME_PREFIX
            );
            return Err(Error::Unathorized);
        }

        let token = &auth_header[SCHEME_PREFIX.len()..];

        let jwt =
            jwt::Token::<jwt::Header, AuthUserClaims, _>::parse_unverified(token).map_err(|e| {
                log::debug!("JWT parsing error: {:?}", e);
                Error::Unathorized
            })?;

        let hmac = Hmac::<Sha384>::new_from_slice(ctx.config.hmac_key.as_bytes())
            .expect("HMAC-SHA-384 can accept any key length");

        // When choosing a JWT implementation, be sure to check that it validates that the signing
        // algorithm declared in the token matches the signing algorithm you're verifying with.
        // The `jwt` crate does.
        let jwt = jwt.verify_with_key(&hmac).map_err(|e| {
            log::debug!("JWT verification error: {:?}", e);
            Error::Unathorized
        })?;

        let (_header, claims) = jwt.into();

        // Because JWTs are stateless, we don't really have any mechanism here to invalidate them
        // besides expiration. You probably want to add more checks, like ensuring the user ID
        // exists and has not been deleted/banned/deactivated.
        //
        // You could also use the user's password hash as part of the keying material for the HMAC,
        // so changing their password invalidates their existing sessions.
        //
        // In practice, Launchbadge has abandoned using JWTs for authenticating long-lived sessions,
        // instead storing session data in Redis, which can be accessed quickly and so adds less
        // overhead to every request compared to hitting Postgres, and allows tracking and
        // invalidating individual sessions by simply deleting them from Redis.
        //
        // Technically, the Realworld spec isn't all that adamant about using JWTs and there
        // may be some flexibility in using other kinds of tokens, depending on whether the frontend
        // is expected to parse the token or just treat it as an opaque string.
        //
        // Also, if the consumer of your API is a browser, you probably want to put your session
        // token in a cookie instead of the response body. By setting the `HttpOnly` flag, the cookie
        // isn't exposed in the response to Javascript at all which, along with setting `Domain` and
        // `SameSite`, prevents all kinds of session hijacking exploits.
        //
        // This also has the benefit of avoiding having to deal with securely storing the session
        // token on the frontend.

        if (claims.exp < OffsetDateTime::now_utc().unix_timestamp()) {
            log::debug!("Token expired");
            return Err(Error::Unathorized);
        }

        let query = sqlx::query!(
            r#"
                select user_id from "user" where user_id = $1
            "#,
            claims.user_id
        );

        let user = query
            .fetch_optional(&ctx.db)
            .await
            .map_err(|e| {
                log::error!("Database error: {:?}", e);
                Error::Unathorized
            })?
            .ok_or_else(|| Error::Unathorized)?;

        Ok(Self {
            user_id: user.user_id,
        })
    }
}

impl MaybeAuthUser {
    pub fn user_id(&self) -> Option<Uuid> {
        self.0.as_ref().map(|x| x.user_id)
    }
}

// tower-http has a `RequireAuthorizationLayer` but it's useless for practical applications,
// as it only supports matching Basic or Bearer auth with credentials you provide it.
//
// There's the `::custom()` constructor to provide your own validator but it basically
// requires parsing the `Authorization` header by-hand anyway so you really don't get anything
// out of it that you couldn't write your own middleware for, except with a bunch of extra
#[async_trait]
impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Extension(ctx) = Extension::<ApiContext>::from_request_parts(parts, state)
            .await
            .expect("BUG: ApiContext was not added as an extension");

        // Get the value of the `Authorization` header, if it was sent at all.
        let auth_header = parts.headers.get(AUTHORIZATION).ok_or(Error::Unathorized)?;

        Self::from_authorization(&ctx, auth_header).await
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for MaybeAuthUser
where
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Extension(ctx) = Extension::<ApiContext>::from_request_parts(parts, state)
            .await
            .expect("BUG: ApiContext was not added as an extension");

        let auth_header = parts.headers.get(AUTHORIZATION);

        let auth_user = match auth_header {
            Some(auth_header) => {
                let auth_user = AuthUser::from_authorization(&ctx, auth_header).await?;
                Some(auth_user)
            }
            None => None,
        };

        Ok(Self(auth_user))
    }
}
