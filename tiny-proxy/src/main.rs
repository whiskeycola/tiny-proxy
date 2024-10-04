use tiny_proxy_core::Proxy;

#[tokio::main]
async fn main() {
    let proxy = Proxy::default().add_user("dima", "qwe123");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    println!("{}", listener.local_addr().unwrap());

    loop {
        let (socket, _) = listener.accept().await.unwrap();

        let proxy = proxy.clone();
        tokio::spawn(async move {
            if let Err(e) = proxy.run(socket).await {
                println!("{e}");
            }
        });
    }
}
