use env_logger::{Builder, Env};
use std::io::Write;
//use std::net::TcpListener;

use rusty::server::Server;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut builder = Builder::from_env(Env::default().default_filter_or("debug"));
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

    /*
    let listener = TcpListener::bind("0.0.0.0:8090").unwrap();
    log::info!(
        "listening on {}",
        listener
            .local_addr()
            .expect("Failed to listen to local addr")
    );
    rusty::run(listener).expect("rusty run failed");
    */

    let mut server = Server::new("0.0.0.0:8090").expect("Failed to start server");
    server.run().expect("server run failed");

    Ok(())
}
