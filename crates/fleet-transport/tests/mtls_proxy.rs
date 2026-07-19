//! Phase 8.5.2: MtlsProxy 엔드투엔드 통합 테스트.
//!
//! 평문 TCP echo upstream → MtlsProxy → WsConn::connect_mtls 클라이언트.
//! TLS 종단 + 클라이언트 인증서 검증 + 양방향 복사가 모두 동작하는지 확인.
//!
//! `--features mtls` 필요.

#![cfg(feature = "mtls")]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
};
use rustls::ServerConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;

use fleet_transport::acp::transport::WsConn;
use fleet_transport::mtls_proxy::MtlsProxy;
use fleet_transport::tls::{ClientTlsConfig, ServerTlsConfig};

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fleet-mtls-proxy-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_pem(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, content).unwrap();
    p
}

struct TestMaterial {
    dir: PathBuf,
    ca_pem: String,
    server_cert_pem: String,
    server_key_pem: String,
    client_cert_pem: String,
    client_key_pem: String,
}

fn generate_material() -> TestMaterial {
    let dir = temp_dir();

    // CA.
    let mut ca_params = CertificateParams::new(vec![]).unwrap();
    ca_params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let mut ca_dn = DistinguishedName::new();
    ca_dn.push(DnType::CommonName, "fleet-test-ca");
    ca_params.distinguished_name = ca_dn;
    let ca_key = KeyPair::generate().unwrap();
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let ca_pem = ca_cert.pem();

    // Server cert.
    let mut server_params =
        CertificateParams::new(vec!["localhost".to_string()]).unwrap();
    let mut sdn = DistinguishedName::new();
    sdn.push(DnType::CommonName, "localhost");
    server_params.distinguished_name = sdn;
    server_params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ServerAuth);
    let server_key = KeyPair::generate().unwrap();
    let server_cert = server_params
        .signed_by(&server_key, &ca_cert, &ca_key)
        .unwrap();
    let server_cert_pem = server_cert.pem();
    let server_key_pem = server_key.serialize_pem();

    // Client cert.
    let mut client_params =
        CertificateParams::new(vec!["orchestrator".to_string()]).unwrap();
    let mut cdn = DistinguishedName::new();
    cdn.push(DnType::CommonName, "orchestrator");
    client_params.distinguished_name = cdn;
    client_params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ClientAuth);
    let client_key = KeyPair::generate().unwrap();
    let client_cert = client_params
        .signed_by(&client_key, &ca_cert, &ca_key)
        .unwrap();
    let client_cert_pem = client_cert.pem();
    let client_key_pem = client_key.serialize_pem();

    TestMaterial {
        dir,
        ca_pem,
        server_cert_pem,
        server_key_pem,
        client_cert_pem,
        client_key_pem,
    }
}

/// 평문 TCP echo upstream. 각 연결마다 받은 바이트를 그대로 되돌려 보낸다.
async fn start_plain_echo_upstream() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut tcp, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                loop {
                    match tcp.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if tcp.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
    });
    addr
}

/// 평문 TCP WebSocket echo upstream. MtlsProxy 가 WebSocket 업그레이드를
/// 그대로 통과시키는지 검증하기 위해 사용.
async fn start_ws_echo_upstream() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let Ok(mut ws) = tokio_tungstenite::accept_async(tcp).await else {
                    return;
                };
                while let Some(Ok(msg)) = ws.next().await {
                    match msg {
                        Message::Text(t) => {
                            if ws.send(Message::Text(t)).await.is_err() {
                                break;
                            }
                        }
                        Message::Close(_) => break,
                        _ => {}
                    }
                }
            });
        }
    });
    addr
}

#[tokio::test]
async fn mtls_proxy_forwards_plain_tcp_roundtrip() {
    let material = generate_material();
    let upstream = start_plain_echo_upstream().await;

    let server_tls = ServerTlsConfig::from_paths(
        write_pem(&material.dir, "ca.pem", &material.ca_pem),
        write_pem(&material.dir, "server.pem", &material.server_cert_pem),
        write_pem(&material.dir, "server.key", &material.server_key_pem),
    );
    let server_config: Arc<ServerConfig> =
        Arc::new(server_tls.build_server_config().unwrap());

    let proxy_addr_unused: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let proxy = MtlsProxy::bind(proxy_addr_unused, upstream, server_config)
        .await
        .expect("proxy bind");
    let proxy_addr = proxy.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let proxy_handle = tokio::spawn(async move { proxy.run(shutdown_rx).await });

    // 클라이언트 구성.
    let client_tls = ClientTlsConfig::from_paths(
        write_pem(&material.dir, "ca.pem", &material.ca_pem),
        write_pem(&material.dir, "client.pem", &material.client_cert_pem),
        write_pem(&material.dir, "client.key", &material.client_key_pem),
    );

    // 평문 TLS TCP 연결 (WebSocket이 아닌 raw TCP) — MtlsProxy 가 비-WebSocket
    // 트래픽도 단순 forward 함을 검증.
    let connector = client_tls.build_connector().unwrap();
    let tcp = tokio::net::TcpStream::connect(proxy_addr)
        .await
        .expect("connect proxy");
    use rustls::pki_types::ServerName;
    let server_name = ServerName::try_from("localhost").unwrap();
    let mut tls = connector.connect(server_name, tcp).await.expect("TLS connect");

    tls.write_all(b"hello mtls").await.unwrap();
    let mut buf = [0u8; 32];
    let n = tokio::time::timeout(Duration::from_secs(3), tls.read(&mut buf))
        .await
        .expect("read timeout")
        .expect("read err");
    assert_eq!(&buf[..n], b"hello mtls");

    let _ = shutdown_tx.send(true);
    let _ = proxy_handle.await;
}

#[tokio::test]
async fn mtls_proxy_forwards_websocket_handshake() {
    let material = generate_material();
    let upstream = start_ws_echo_upstream().await;

    let server_tls = ServerTlsConfig::from_paths(
        write_pem(&material.dir, "ca.pem", &material.ca_pem),
        write_pem(&material.dir, "server.pem", &material.server_cert_pem),
        write_pem(&material.dir, "server.key", &material.server_key_pem),
    );
    let server_config = Arc::new(server_tls.build_server_config().unwrap());

    let proxy_addr_unused: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let proxy = MtlsProxy::bind(proxy_addr_unused, upstream, server_config)
        .await
        .expect("proxy bind");
    let proxy_addr = proxy.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let proxy_handle = tokio::spawn(async move { proxy.run(shutdown_rx).await });

    let client_tls = ClientTlsConfig::from_paths(
        write_pem(&material.dir, "ca.pem", &material.ca_pem),
        write_pem(&material.dir, "client.pem", &material.client_cert_pem),
        write_pem(&material.dir, "client.key", &material.client_key_pem),
    );

    let url = format!("wss://localhost:{}/ws?server-key=x", proxy_addr.port());
    let (ws, mut reader) = WsConn::connect_mtls(&url, &client_tls)
        .await
        .expect("mTLS connect through proxy");

    ws.send_text(r#"{"jsonrpc":"2.0","method":"ping","id":1}"#)
        .await
        .unwrap();

    let msg = tokio::time::timeout(Duration::from_secs(5), reader.next())
        .await
        .expect("timeout")
        .expect("stream closed")
        .expect("ws err");

    match msg {
        Message::Text(t) => assert!(t.contains("ping")),
        other => panic!("expected text, got {other:?}"),
    }

    let _ = ws.close().await;
    let _ = shutdown_tx.send(true);
    let _ = proxy_handle.await;
}

#[tokio::test]
async fn mtls_proxy_rejects_client_with_untrusted_cert() {
    let material = generate_material();
    let upstream = start_plain_echo_upstream().await;

    let server_tls = ServerTlsConfig::from_paths(
        write_pem(&material.dir, "ca.pem", &material.ca_pem),
        write_pem(&material.dir, "server.pem", &material.server_cert_pem),
        write_pem(&material.dir, "server.key", &material.server_key_pem),
    );
    let server_config = Arc::new(server_tls.build_server_config().unwrap());

    let proxy_addr_unused: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let proxy = MtlsProxy::bind(proxy_addr_unused, upstream, server_config)
        .await
        .expect("proxy bind");
    let proxy_addr = proxy.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let proxy_handle = tokio::spawn(async move { proxy.run(shutdown_rx).await });

    // Rogue CA + rogue client cert.
    let mut rogue_params = CertificateParams::new(vec![]).unwrap();
    rogue_params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let mut rdn = DistinguishedName::new();
    rdn.push(DnType::CommonName, "rogue-ca");
    rogue_params.distinguished_name = rdn;
    let rogue_key = KeyPair::generate().unwrap();
    let rogue_ca = rogue_params.self_signed(&rogue_key).unwrap();
    let rogue_ca_pem = rogue_ca.pem();

    let mut client_params =
        CertificateParams::new(vec!["orchestrator".to_string()]).unwrap();
    let mut cdn = DistinguishedName::new();
    cdn.push(DnType::CommonName, "orchestrator");
    client_params.distinguished_name = cdn;
    client_params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ClientAuth);
    let client_key = KeyPair::generate().unwrap();
    let client_cert = client_params
        .signed_by(&client_key, &rogue_ca, &rogue_key)
        .unwrap();

    let client_tls = ClientTlsConfig::from_paths(
        write_pem(&material.dir, "rogue-ca.pem", &rogue_ca_pem),
        write_pem(&material.dir, "rogue-client.pem", &client_cert.pem()),
        write_pem(&material.dir, "rogue-client.key", &client_key.serialize_pem()),
    );

    let connector = client_tls.build_connector().unwrap();
    let tcp = tokio::net::TcpStream::connect(proxy_addr)
        .await
        .expect("connect proxy");
    use rustls::pki_types::ServerName;
    let server_name = ServerName::try_from("localhost").unwrap();
    let result = connector.connect(server_name, tcp).await;
    assert!(result.is_err(), "untrusted client cert must be rejected");
    let _ = shutdown_tx.send(true);
    let _ = proxy_handle.await;
}
