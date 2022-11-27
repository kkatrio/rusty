use env_logger::Builder;
use std::io::Write;
use std::net::TcpListener;

fn main() {
    let mut builder = Builder::from_env("LOG_LEVEL");
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
    //use env_logger::Env;
    let env = Env::default()
        .filter_or("LOG_LEVEL", "trace")
        .write_style_or("LOG_STYLE", "always");
    env_logger::init_from_env(env);
    */

    let listener = TcpListener::bind("0.0.0.0:8090").unwrap();
    log::info!(
        "listening on {}",
        listener
            .local_addr()
            .expect("Failed to listen to local addr")
    );

    rusty::run(listener).expect("rusty run failed");
}
