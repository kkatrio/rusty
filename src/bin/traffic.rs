async fn send_request() {
    for _ in 0..2 {
        let body = reqwest::get("http://127.0.0.1:8090")
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        println!("body = {:?}", body);
    }
}

async fn many_requests_one_connection() {
    let client = reqwest::Client::new();

    for _ in 0..1000 {
        let status = client
            .get("http://127.0.0.1:8090")
            .send()
            .await
            .unwrap()
            .status();
        assert!(status.is_success());
    }
}

#[tokio::main]
async fn main() {
    println!("traffic generator");
    send_request().await;
    many_requests_one_connection().await;
}
