//! `crm-server`: identity for Breksos CRM. Configure with the environment:
//!
//!   DATABASE_URL        postgres://user:pass@host/db   (required)
//!   BIND                address to listen on, default 127.0.0.1:8787
//!   CRM_REQUIRE_TLS     1 → refuse any request that did not arrive as https (behind a proxy)
//!   CRM_TRUST_PROXY     1 → trust N proxies you control (1 = one): the client is the Nth entry from the right of
//!                         X-Forwarded-For. Unset: the connection's own address.
//!
//! It never logs a request, a header or a body.

use crm_server::{App, Config};
use std::str::FromStr;

#[tokio::main]
async fn main() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("crm-server: DATABASE_URL is not set");
        std::process::exit(1);
    };
    let bind = std::env::var("BIND").unwrap_or_else(|_| "127.0.0.1:8787".to_string());
    let pg = match tokio_postgres::Config::from_str(&url) {
        Ok(c) => c,
        // The error text can quote the URL, which holds the password: say only that it is wrong.
        Err(_) => {
            eprintln!("crm-server: DATABASE_URL is not a valid postgres URL");
            std::process::exit(1);
        }
    };
    let mgr = deadpool_postgres::Manager::new(pg, tokio_postgres::NoTls);
    let pool = deadpool_postgres::Pool::builder(mgr).max_size(16).build().expect("pool");
    let config = Config::from_env();
    let app = match App::new(pool, config.clone()).await {
        Ok(a) => a,
        Err(_) => {
            eprintln!("crm-server: could not reach or migrate the database");
            std::process::exit(1);
        }
    };
    app.start_background();
    if !config.require_tls {
        eprintln!("crm-server: WARNING — CRM_REQUIRE_TLS is off: this server will answer over plain HTTP. Fine on localhost; never expose it so.");
    }
    let listener = match tokio::net::TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(_) => {
            eprintln!("crm-server: cannot listen on {bind}");
            std::process::exit(1);
        }
    };
    eprintln!("crm-server: listening on {bind}");
    let _ = axum::serve(listener, app.router().into_make_service_with_connect_info::<std::net::SocketAddr>()).await;
}
