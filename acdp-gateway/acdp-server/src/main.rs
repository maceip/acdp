//! ACDP Gateway Server
//!
//! Implements the Agent Credential Delegation Protocol (ACDP) v0.3.
//! Integrates with Rauthy for OIDC/OAuth2 and mcp-auth for cryptographic credentials.

mod models;
mod routes;
mod services;

use actix_web::{middleware, web, App, HttpServer};
use acdp_common::Result;
use sqlx::postgres::PgPoolOptions;
use std::env;
use tracing::info;

#[derive(Clone)]
pub struct AppState {
    pub db_pool: sqlx::PgPool,
    pub config: Config,
    pub credential_service: services::credential::CredentialService,
    pub rauthy_client: services::rauthy_client::RauthyClient,
}

#[derive(Clone)]
pub struct Config {
    pub server_host: String,
    pub server_port: u16,
    pub database_url: String,
    pub rauthy_base_url: String,
    pub rauthy_admin_token: String,
    pub gateway_issuer: String,
    pub gateway_signing_key: Vec<u8>,
    pub gateway_public_key: Vec<u8>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        Ok(Self {
            server_host: env::var("ACDP_SERVER_HOST")
                .unwrap_or_else(|_| "127.0.0.1".to_string()),
            server_port: env::var("ACDP_SERVER_PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()
                .expect("Invalid ACDP_SERVER_PORT"),
            database_url: env::var("DATABASE_URL")
                .expect("DATABASE_URL must be set"),
            rauthy_base_url: env::var("RAUTHY_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:8000".to_string()),
            rauthy_admin_token: env::var("RAUTHY_ADMIN_TOKEN")
                .expect("RAUTHY_ADMIN_TOKEN must be set"),
            gateway_issuer: env::var("ACDP_GATEWAY_ISSUER")
                .unwrap_or_else(|_| "https://acdp-gateway.kontext.dev/".to_string()),
            gateway_signing_key: hex::decode(
                env::var("ACDP_GATEWAY_SIGNING_KEY")
                    .expect("ACDP_GATEWAY_SIGNING_KEY must be set (hex-encoded Ed25519 secret)")
            ).expect("Invalid ACDP_GATEWAY_SIGNING_KEY hex encoding"),
            gateway_public_key: hex::decode(
                env::var("ACDP_GATEWAY_PUBLIC_KEY")
                    .expect("ACDP_GATEWAY_PUBLIC_KEY must be set (hex-encoded Ed25519 public)")
            ).expect("Invalid ACDP_GATEWAY_PUBLIC_KEY hex encoding"),
        })
    }
}

#[actix_web::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    info!("Starting ACDP Gateway Server...");

    // Load configuration
    let config = Config::from_env()?;
    info!("Configuration loaded");

    // Connect to database
    let db_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&config.database_url)
        .await?;
    info!("Database connected");

    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&db_pool)
        .await
        .expect("Failed to run database migrations");
    info!("Database migrations complete");

    // Initialize services
    let credential_service = services::credential::CredentialService::new(
        db_pool.clone(),
        config.gateway_signing_key.clone(),
        config.gateway_public_key.clone(),
        config.gateway_issuer.clone(),
    );

    let rauthy_client = services::rauthy_client::RauthyClient::new(
        config.rauthy_base_url.clone(),
        config.rauthy_admin_token.clone(),
    );

    let app_state = AppState {
        db_pool,
        config: config.clone(),
        credential_service,
        rauthy_client,
    };

    info!("Services initialized");

    // Start HTTP server
    let bind_addr = format!("{}:{}", config.server_host, config.server_port);
    info!("Starting server on {}", bind_addr);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(app_state.clone()))
            .wrap(middleware::Logger::default())
            .wrap(middleware::Compress::default())
            .service(
                web::scope("/acdp/v1")
                    .service(routes::credential_issue::issue_credential)
                    .service(routes::credential_verify::verify_credential)
                    .service(routes::delegation::delegate_credential),
            )
            .service(routes::health::health_check)
    })
    .bind(&bind_addr)?
    .run()
    .await?;

    Ok(())
}
