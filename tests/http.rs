use env_logger::{Builder, Env};
use reqwest;
use std::io::Write;
use std::net::TcpListener;

async fn spawn_server() {
    //let mut builder = Builder::from_env("LOG_LEVEL");
    let mut builder = Builder::from_env(Env::default().default_filter_or("trace"));
    builder.format(|buf, record| {
        writeln!(
            buf,
            "{:5} | {:>15}:{:<4} | {} | {}",
            record.level(),
            record.file().unwrap_or("unknown file"),
            record.line().unwrap_or(0),
            buf.timestamp(),
            record.args()
        )
    });
    builder.init();

    let listener = TcpListener::bind("0.0.0.0:8090").unwrap();
    log::info!(
        "listening on {}",
        listener
            .local_addr()
            .expect("Failed to listen to local addr")
    );

    let server = rusty::run(listener);
    tokio::spawn(server);
}

//#[async_std::test]
#[tokio::test]
async fn it_works() {
    spawn_server().await;
    log::info!("Request ready");
    let client = reqwest::Client::new();
    let status = client
        .get("http://0.0.0.0:8090")
        .send()
        .await
        .expect("Cannot do get request")
        .status();

    log::info!("status: {}", status);
    //let response = client
    //    .get("http://0.0.0.0:8090")
    //    .send()
    //    .await
    //    .expect("Url cannot be parsed");
    log::info!("Request done");
    //assert!(response.status().is_success());
}
