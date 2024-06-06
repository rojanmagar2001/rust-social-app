// use crate::http::{Error, ResultExt};
// use axum::{
//     async_trait,
//     body::HttpBody,
//     extract::{FromRequestParts, Request},
//     BoxError, Form, Json,
// };
// use serde::de::DeserializeOwned;
// use validator::Validate;

// /// use this to encapsulate fields that require validation
// pub struct ValidatedJson<J>(pub J);

// #[async_trait]
// impl<S, B, J> FromRequestParts<S, B> for ValidatedJson<J>
// where
//     B: Send + 'static,
//     S: Send + Sync,
//     J: Validate + 'static,
//     Json<J>: FromRequestParts<S, B>,
// {
//     type Rejection = Error;

//     async fn from_request_parts(
//         parts: &mut Request<B>,
//         state: &S,
//     ) -> Result<Self, Self::Rejection> {
//         let Json(data) = Json::from_request_parts(parts, state)
//             .await
//             .map_err(|_| Error::unprocessable_entity(["json validation", "error"]))?;
//         data.validate()
//             .map_err(|_| Error::unprocessable_entity(["json validation", "error"]))?;
//         Ok(Self(data))
//     }
// }
