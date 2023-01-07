use env_logger::{Builder, Env};
use reqwest;
use rusty::server::Server;
use std::io::{self, Write};
use std::process::Command;
use std::thread;
use tokio::task;

async fn spawn_server() {
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

    let mut server = Server::new("0.0.0.0:8090").expect("Failed to start server");
    server.run().expect("server run failed");
}

/* run blocks on poll.
 * The connection is not accepted when attempted with reqwest async client.
 * It fails when using spawn_blocking, and blocks in poll when using tokio::spawn
 */
#[tokio::test]
async fn reqwest_client() {
    let server = spawn_server();
    //task::spawn_blocking(|| server);
    thread::spawn(|| server);

    log::info!("Server ready");
    let client = reqwest::Client::new();

    // poll blocks -- socket never ready
    let status = client
        .get("http://127.0.0.1:8090")
        .send()
        .await
        .expect("Cannot do get request")
        .status();
    log::info!("status: {}", status);
    log::info!("Request done");
}

#[tokio::test]
async fn tcp_netcat() {
    let server = spawn_server();
    tokio::spawn(server);
    log::info!("Server ready");

    // connection never accepted
    let output = Command::new("nc")
        .arg("127.0.0.1")
        .arg("8090")
        .output()
        .expect("Failed to execute command");
    io::stdout().write_all(&output.stdout).unwrap();
    io::stderr().write_all(&output.stderr).unwrap();
    log::info!("Request done");
}
