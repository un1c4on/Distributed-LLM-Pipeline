use axum::{
    extract::Json,
    response::sse::{Event, Sse},
    routing::post,
    Router,
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::AsyncReadExt,
    process::Command,
    sync::mpsc,
};
use tokio_stream::wrappers::ReceiverStream;
use tower_http::{cors::CorsLayer, services::ServeDir};

#[derive(Deserialize)]
struct ChatRequest {
    prompt: String,
}

#[derive(Serialize, Clone)]
struct StreamMessage {
    msg_type: String,
    content: String,
}

async fn chat_handler(
    Json(payload): Json<ChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, axum::Error>>> {
    println!("📩 İstek: {}", payload.prompt);
    let (tx, rx) = mpsc::channel(200);

    let mut child = Command::new("stdbuf")
        .arg("-o0")
        .arg("-e0")
        .arg("/home/un1c4on/Dist_LLM/llama.cpp/build/bin/llama-cli")
        .arg("-m")
        .arg("/home/un1c4on/Dist_LLM/baronllm-llama3.1-v1-q6_k.gguf")
        .arg("-p")
        .arg(&payload.prompt)
        .arg("-n")
        .arg("200")
        .arg("-c")
        .arg("2048")
        .arg("--rpc")
        .arg("127.0.0.1:50052,127.0.0.1:50053")
        .arg("-ngl")
        .arg("99")
        .arg("--verbose")
        .arg("--log-file")            // EKLENDİ: Logları dosyaya zorla
        .arg("system_log.txt")        // EKLENDİ: Dosya adı
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Llama başlatılamadı");

    let mut stdout = child.stdout.take().expect("stdout yakalanamadı");
    let mut stderr = child.stderr.take().expect("stderr yakalanamadı");

    // LOGLARI OKU (HEM TERMİNALE HEM WEB'E)
    let tx_log = tx.clone();
    tokio::spawn(async move {
        let mut buffer = [0; 1024];
        while let Ok(n) = stderr.read(&mut buffer).await {
            if n == 0 { break; }
            let content = String::from_utf8_lossy(&buffer[..n]).to_string();
            
            // BURASI ÖNEMLİ: Logları anında terminale basıyoruz
            use std::io::Write;
            print!("{}", content); 
            std::io::stdout().flush().unwrap();

            let msg = StreamMessage { msg_type: "log".to_string(), content };
            if let Ok(json) = serde_json::to_string(&msg) {
                let _ = tx_log.send(Ok(Event::default().data(json))).await;
            }
        }
    });

    // TOKENS OKU
    let tx_token = tx.clone();
    tokio::spawn(async move {
        let mut buffer = [0; 64];
        while let Ok(n) = stdout.read(&mut buffer).await {
            if n == 0 { break; }
            let content = String::from_utf8_lossy(&buffer[..n]).to_string();
            let msg = StreamMessage { msg_type: "token".to_string(), content };
            if let Ok(json) = serde_json::to_string(&msg) {
                let _ = tx_token.send(Ok(Event::default().data(json))).await;
            }
        }
        let _ = child.wait().await;
    });

    Sse::new(ReceiverStream::new(rx)).keep_alive(axum::response::sse::KeepAlive::new().interval(Duration::from_secs(1)))
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/chat", post(chat_handler))
        .fallback_service(ServeDir::new("static"))
        .layer(CorsLayer::permissive());

    let addr = "0.0.0.0:3005";
    println!("🚀 Çift Worker Sunucusu Hazır: http://localhost:3005");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
