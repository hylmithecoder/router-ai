//! Request handlers.
//!
//! Each handler is an async function that receives Axum extractors and returns something
//! implementing `IntoResponse`. Handlers should be thin: validate input, call a service or
//! repository, and format the response.

pub mod admin;
pub mod chat;
pub mod health;
pub mod ocr;
