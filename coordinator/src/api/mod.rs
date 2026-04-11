pub mod grpc;
pub mod rest;

use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub nats: Option<async_nats::Client>,
    pub redis: Option<redis::Client>,
}
