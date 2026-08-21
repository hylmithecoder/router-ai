//! # Rust Axum Backend Template
//!
//! This crate is a starting point for backend services built with:
//! - [Axum](https://docs.rs/axum) for HTTP routing and middleware
//! - [Tokio](https://docs.rs/tokio) for the async runtime
//! - [Tower](https://docs.rs/tower) / [tower-http](https://docs.rs/tower-http) for reusable middleware
//!
//! The layout intentionally splits responsibilities into small modules so that an LLM
//! (or any contributor) can understand and extend one piece at a time.
//!
//! ## Module map
//! - `config`: reads environment variables and `.env` files.
//! - `error`: a single `AppError` type that every handler can return.
//! - `state`: shared application state injected into requests.
//! - `middleware`: reusable Tower/Axum middleware.
//! - `routes`: route definitions and nesting.
//! - `handlers`: request handlers (thin; delegate to services/models).
//! - `models`: request/response DTOs and domain types.
//! - `server`: TCP listener setup and graceful shutdown.

pub mod ai;
pub mod config;
pub mod database;
pub mod error;
pub mod handlers;
pub mod middleware;
pub mod models;
pub mod response;
pub mod routes;
pub mod server;
pub mod state;
pub mod utils;
