use std::{borrow::Cow, collections::HashMap};

use axum::{
    body::{Body, Bytes, HttpBody},
    http::{Response, StatusCode},
    response::IntoResponse,
    Json,
};
use sqlx::error::DatabaseError;
use tower_http::body::Full;

/// A common error type that can be used throughout the API.
///
/// Can be returned in a `Result` from an API handler function.
///
/// For convenience, this represents both API errors as well as internal recoverable errors,
/// and maps them to appropriate status codes along with at least a minimally useful error
/// message in a plain text body, or a JSON body in the case of `UnprocessableEntity`.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    // Return `401 Unathorized` when the user is not authenticated.
    #[error("Authentication Required")]
    Unathorized,

    /// Return `403 Forbidden` when the user is authenticated but not authorized to perform the action.
    #[error("User may not perform this action")]
    Forbidden,

    /// Return `404 Not Found` when a resource is not found.
    #[error("Resource path not found")]
    NotFound,

    /// Return `422 Unprocessable Entity`
    ///
    /// This also serializes the `errors` map to JSON to satisfy the requirement for
    /// `422 Unprocessable Entity` errors in the Realworld spec:
    /// https://realworld-docs.netlify.app/docs/specs/backend-specs/error-handling
    ///
    /// For a good API, the other status codes should also ideally map to some sort of JSON body
    /// that the frontend can deal with, but I do admit sometimes I've just gotten lazy and
    /// returned a plain error message if there were few enough error modes for a route
    /// that the frontend could infer the error from the status code alone.
    #[error("Error in the request body")]
    UnprocessableEntity {
        errors: HashMap<Cow<'static, str>, Vec<Cow<'static, str>>>,
    },

    /// Automatically return `500 Internal Server Error` on a `sqlx::Error`.
    ///
    /// Via the generated `From<sqlx::Error> for Error` impl,
    /// this allows using `?` on database calls in handler functions without a manual mapping step.
    ///
    /// I highly recommend creating an error type like this if only to make handler function code
    /// nicer; code in Actix-web projects that we started before I settled on this pattern is
    /// filled with `.map_err(ErrInternalServerError)?` which is a *ton* of unnecessary noise.
    ///
    /// The actual error message isn't returned to the client for security reasons.
    /// It should be logged instead.
    ///
    /// Note that this could also contain database constraint errors, which should usually
    /// be transformed into client errors (e.g. `422 Unprocessable Entity` or `409 Conflict`).
    /// See `ResultExt` below for a convenient way to do this.
    #[error("An error occured with the database")]
    Sqlx(#[from] sqlx::Error),

    /// Return `500 Internal Server Error` on a `anyhow::Error`.
    ///
    /// `anyhow::Error` is used in a few places to capture context and backtraces
    /// on unrecoverable (but technically non-fatal) errors which could be highly useful for
    /// debugging. We use it a lot in our code for background tasks or making API calls
    /// to external services so we can use `.context()` to refine the logged error.
    ///
    /// Via the generated `From<anyhow::Error> for Error` impl, this allows the
    /// use of `?` in handler functions to automatically convert `anyhow::Error` into a response.
    ///
    /// Like with `Error::Sqlx`, the actual error message is not returned to the client
    /// for security reasons.
    #[error("An internal server error occured")]
    Anyhow(#[from] anyhow::Error),
}

impl Error {
    /// Convenient constructor for `Error::UnprocessableEntity`.
    ///
    /// Multiple for the same key are collected into a list for that key.
    ///
    /// Try "Go to Usage" in an IDE for examples.
    pub fn unprocessable_entity<K, V>(errors: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: Into<Cow<'static, str>>,
        V: Into<Cow<'static, str>>,
    {
        let mut error_map = HashMap::new();

        for (key, val) in errors {
            error_map
                .entry(key.into())
                .or_insert_with(Vec::new)
                .push(val.into());
        }

        Self::UnprocessableEntity { errors: error_map }
    }

    fn status_code(&self) -> StatusCode {
        match self {
            Self::Unathorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::UnprocessableEntity { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Sqlx(_) | Self::Anyhow(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(serde::Serialize)]
struct ErrorBody {
    success: bool,
    message: String,
}

/// Axum allows you to return `Result` from handler functions, but the error type
/// also must be some sort of response type.
///
/// By default, the generated `Display` impl is used to return a plaintext error message
/// to the client.
impl IntoResponse for Error {
    fn into_response(self) -> Response<Body> {
        match self {
            Self::UnprocessableEntity { errors } => {
                #[derive(serde::Serialize)]
                struct Errors {
                    success: bool,
                    errors: HashMap<Cow<'static, str>, Vec<Cow<'static, str>>>,
                }

                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(Errors {
                        success: false,
                        errors,
                    }),
                )
                    .into_response();
            }
            _ => (),
        }

        (
            self.status_code(),
            Json(ErrorBody {
                success: false,
                message: self.to_string(),
            }),
        )
            .into_response()
    }
}

/// A little helper trait for more easily converting database constraint errors into API errors.
///
/// ```rust,ignore
/// let user_id = sqlx::query_scalar!(
///     r#"insert into "user" (username, email, password_hash) values ($1, $2, $3) returning user_id"#,
///     username,
///     email,
///     password_hash
/// )
///     .fetch_one(&ctxt.db)
///     .await
///     .on_constraint("user_username_key", |_| Error::unprocessable_entity([("username", "already taken")]))?;
/// ```
///
/// Something like this would ideally live in a `sqlx-axum` crate if it made sense to author one,
/// however its definition is tied pretty intimately to the `Error` type, which is itself
/// tied directly to application semantics.
///
/// To actually make this work in a generic context would make it quite a bit more complex,
/// as you'd need an intermediate error type to represent either a mapped or an unmapped error,
/// and even then it's not clear how to handle `?` in the unmapped case without more boilerplate.
pub trait ResultExt<T> {
    /// If `self` contains a SQLx database constraint error with the given name,
    /// transform the error.
    ///
    /// Otherwise, the result is passed through unchanged.
    fn on_constraint(
        self,
        name: &str,
        f: impl FnOnce(Box<dyn DatabaseError>) -> Error,
    ) -> Result<T, Error>;
}

impl<T, E> ResultExt<T> for Result<T, E>
where
    E: Into<Error>,
{
    fn on_constraint(
        self,
        name: &str,
        map_err: impl FnOnce(Box<dyn DatabaseError>) -> Error,
    ) -> Result<T, Error> {
        self.map_err(|e| match e.into() {
            Error::Sqlx(sqlx::Error::Database(dbe)) if dbe.constraint() == Some(name) => {
                map_err(dbe)
            }
            e => e,
        })
    }
}
