//! Phase 8.5: WsConn mTLS 통합 테스트.
//!
//! 이 테스트는 실제 TLS 핸드셰이크를 수행한다:
//! 1. rcgen 으로 ephemeral CA + 서버 인증서 + 클라이언트 인증서를 발급.
//! 2. 사설 CA로 서명된 서버 인증서 + 클라이언트 인증서를 요구하는 rustls 서버 구동.
//! 3. `WsConn::connect_mtls` 로 접속 → 텍스트 프레임을 주고받는다.
//!
//! `--features mtls` 필요.

#![cfg(feature = "mtls")]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rcgen::{CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use rustls_pemfile::certs;
use std::io::BufReader;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::tungstenite::Message;

use fleet_transport::acp::transport::WsConn;
use fleet_transport::tls::ClientTlsConfig;

/// ephemeral 테스트 디렉토리. 프로세스 종료 시 OS가 정리 (tmp).
fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fleet-mtls-test-{}-{}",
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

/// pem 문자열에서 CertificateDer 벡터 추출.
fn pem_to_certs(pem: &str) -> Vec<CertificateDer<'static>> {
    let mut reader = BufReader::new(pem.as_bytes());
    certs(&mut reader).collect::<Result<Vec<_>, _>>().unwrap()
}

fn pem_to_key(pem: &str) -> PrivateKeyDer<'static> {
    let mut reader = BufReader::new(pem.as_bytes());
    rustls_pemfile::private_key(&mut reader).unwrap().unwrap()
}

struct TestMaterial {
    dir: PathBuf,
    ca_pem: String,
    server_cert_pem: String,
    server_key_pem: String,
    client_cert_pem: String,
    client_key_pem: String,
}

/// rcgen 0.13 API 로 ephemeral CA + 서버/클라이언트 인증서 발급.
fn generate_material() -> TestMaterial {
    let dir = temp_dir();

    // 1. CA (self-signed).
    let mut ca_params = CertificateParams::new(vec![]).unwrap();
    ca_params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let mut ca_dn = DistinguishedName::new();
    ca_dn.push(DnType::CommonName, "fleet-test-ca");
    ca_params.distinguished_name = ca_dn;
    ca_params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::Any);
    let ca_key = KeyPair::generate().unwrap();
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let ca_pem = ca_cert.pem();

    // 2. Server cert (signed by CA, SAN=localhost).
    let mut server_params = CertificateParams::new(vec!["localhost".to_string()]).unwrap();
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

    // 3. Client cert (signed by CA).
    let mut client_params = CertificateParams::new(vec!["orchestrator".to_string()]).unwrap();
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

/// TLS WebSocket 서버 (클라이언트 인증서 강제) 구동.
/// 각 연결마다 echo 핸들러 실행.
async fn start_mtls_server(material: &TestMaterial) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    // Server config: server cert + client cert required (signed by same CA).
    let server_chain = pem_to_certs(&material.server_cert_pem);
    let server_key = pem_to_key(&material.server_key_pem);

    let ca_certs = pem_to_certs(&material.ca_pem);
    let mut client_roots = RootCertStore::empty();
    for c in &ca_certs {
        client_roots.add(c.clone()).unwrap();
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let client_verifier =
        WebPkiClientVerifier::builder_with_provider(Arc::new(client_roots), provider.clone())
            .build()
            .unwrap();

    let server_config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_client_cert_verifier(client_verifier)
        .with_single_cert(server_chain, server_key)
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(server_config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let join = tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                return;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(tcp).await else {
                    return;
                };
                // Echo WebSocket.
                let Ok(mut ws) = tokio_tungstenite::accept_async(tls).await else {
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

    (addr, join)
}

#[tokio::test]
async fn wsconn_connect_mtls_roundtrips_text() {
    let material = generate_material();
    let ca_path = write_pem(&material.dir, "ca.pem", &material.ca_pem);
    let client_cert_path = write_pem(&material.dir, "client.pem", &material.client_cert_pem);
    let client_key_path = write_pem(&material.dir, "client.key", &material.client_key_pem);

    let (addr, _server) = start_mtls_server(&material).await;
    let url = format!("wss://localhost:{}/ws?server-key=x", addr.port());

    let tls = ClientTlsConfig::from_paths(&ca_path, &client_cert_path, &client_key_path);
    let (ws, mut reader) = WsConn::connect_mtls(&url, &tls)
        .await
        .expect("mTLS connect");

    ws.send_text(r#"{"jsonrpc":"2.0","method":"ping","id":1}"#)
        .await
        .unwrap();

    // 응답 대기 (echo).
    let msg = tokio::time::timeout(Duration::from_secs(5), reader.next())
        .await
        .expect("timed out")
        .expect("stream closed")
        .expect("ws error");

    match msg {
        Message::Text(t) => assert!(t.contains("ping")),
        other => panic!("expected text frame, got {other:?}"),
    }

    let _ = ws.close().await;
}

#[tokio::test]
async fn wsconn_connect_mtls_rejects_ws_url() {
    let material = generate_material();
    let ca_path = write_pem(&material.dir, "ca.pem", &material.ca_pem);
    let client_cert_path = write_pem(&material.dir, "client.pem", &material.client_cert_pem);
    let client_key_path = write_pem(&material.dir, "client.key", &material.client_key_pem);
    let tls = ClientTlsConfig::from_paths(&ca_path, &client_cert_path, &client_key_path);

    let result = WsConn::connect_mtls("ws://localhost:1234/ws", &tls).await;
    assert!(result.is_err(), "ws:// must be rejected for connect_mtls");
    let err = match result {
        Err(e) => e,
        _ => unreachable!(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("wss://"), "unexpected: {msg}");
}

#[tokio::test]
async fn wsconn_connect_mtls_fails_with_untrusted_client_cert() {
    // 서로 다른 CA로 발급한 클라이언트 인증서 → 서버가 거부해야 함.
    let server_material = generate_material();

    // 별도 CA로 클라이언트 인증서 발급 (신뢰하지 않는 CA).
    let mut rogue_ca_params = CertificateParams::new(vec![]).unwrap();
    rogue_ca_params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "rogue-ca");
    rogue_ca_params.distinguished_name = dn;
    let rogue_ca_key = KeyPair::generate().unwrap();
    let rogue_ca = rogue_ca_params.self_signed(&rogue_ca_key).unwrap();
    let rogue_ca_pem = rogue_ca.pem();

    let mut client_params = CertificateParams::new(vec!["orchestrator".to_string()]).unwrap();
    let mut cdn = DistinguishedName::new();
    cdn.push(DnType::CommonName, "orchestrator");
    client_params.distinguished_name = cdn;
    client_params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ClientAuth);
    let client_key = KeyPair::generate().unwrap();
    let client_cert = client_params
        .signed_by(&client_key, &rogue_ca, &rogue_ca_key)
        .unwrap();
    let client_cert_pem = client_cert.pem();
    let client_key_pem = client_key.serialize_pem();

    let dir = temp_dir();
    let rogue_ca_path = write_pem(&dir, "rogue-ca.pem", &rogue_ca_pem);
    let client_cert_path = write_pem(&dir, "client.pem", &client_cert_pem);
    let client_key_path = write_pem(&dir, "client.key", &client_key_pem);

    let (addr, _server) = start_mtls_server(&server_material).await;
    let url = format!("wss://localhost:{}/ws", addr.port());

    let tls = ClientTlsConfig::from_paths(&rogue_ca_path, &client_cert_path, &client_key_path);
    let result = WsConn::connect_mtls(&url, &tls).await;
    assert!(
        result.is_err(),
        "untrusted client cert must be rejected by server"
    );
    let err = match result {
        Err(e) => e,
        _ => unreachable!(),
    };
    let msg = format!("{err}").to_lowercase();
    // alert / handshake 실패 메시지가 포함되어야 함.
    assert!(
        msg.contains("handshake") || msg.contains("alert") || msg.contains("invalid"),
        "unexpected success path; got error: {msg}"
    );
}
