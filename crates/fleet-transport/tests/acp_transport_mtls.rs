//! Phase 8.5.3: AcpTransport mTLS 통합 테스트.
//!
//! `AcpTransport::with_client_tls` 로 구성된 transport 가 `wss://` endpoint 에
//! 대해 mTLS 핸드셰이크를 수행하고, register 가 성공하는지 검증한다. 서버는
//! 클라이언트 인증서를 강제하고, ACP 초기화 (`initialize` + `session/new`)
//! 에 대한 최소한의 JSON-RPC 응답을 반환한다.
//!
//! `--features "acp mtls"` 필요.

#![cfg(all(feature = "acp", feature = "mtls"))]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use rustls_pemfile::certs;
use std::io::BufReader;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::tungstenite::Message;

use fleet_core::WorkerId;
use fleet_transport::acp_transport::AcpTransport;
use fleet_transport::tls::ClientTlsConfig;
use fleet_transport::WorkerTransport;

// ─── 테스트 인증서 생성 (mtls_handshake.rs 와 동일 패턴) ─────────────

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fleet-acp-mtls-{tag}-{}-{}",
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

fn pem_to_certs(pem: &str) -> Vec<CertificateDer<'static>> {
    let mut reader = BufReader::new(pem.as_bytes());
    certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
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

fn generate_material() -> TestMaterial {
    let dir = temp_dir("acp");

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

// ─── mTLS + ACP handshake 서버 ──────────────────────────────────────

/// initialize + session/new JSON-RPC 에 응답하는 minimal ACP 서버.
/// 그 외 메시지는 무시. 클라이언트 인증서 필수.
async fn start_acp_mtls_server(
    material: &TestMaterial,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let server_chain = pem_to_certs(&material.server_cert_pem);
    let server_key = pem_to_key(&material.server_key_pem);

    let ca_certs = pem_to_certs(&material.ca_pem);
    let mut client_roots = RootCertStore::empty();
    for c in &ca_certs {
        client_roots.add(c.clone()).unwrap();
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let client_verifier = WebPkiClientVerifier::builder_with_provider(
        Arc::new(client_roots),
        provider.clone(),
    )
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
                let Ok(mut ws) = tokio_tungstenite::accept_async(tls).await else {
                    return;
                };
                // ACP handshake: initialize -> session/new -> 무한 대기.
                while let Some(Ok(msg)) = ws.next().await {
                    if let Message::Text(t) = msg {
                        // 간이 JSON-RPC router.
                        let response = if t.contains("\"initialize\"") {
                            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-01","capabilities":{}}}"#
                        } else if t.contains("\"session/new\"") {
                            r#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"test-session"}}"#
                        } else {
                            // 그 외 메시지는 무시 (응답하지 않음).
                            continue;
                        };
                        if ws.send(Message::Text(response.to_string())).await.is_err() {
                            break;
                        }
                    } else if let Message::Close(_) = msg {
                        break;
                    }
                }
            });
        }
    });

    (addr, join)
}

// ─── 테스트 ──────────────────────────────────────────────────────────

/// `AcpTransport::with_client_tls` 로 mTLS 를 활성화하고 wss:// endpoint 에
/// register 가 성공하는지 검증. 서버가 정상적으로 initialize + session/new
/// 응답을 반환하면 Connected 상태가 되어야 한다.
#[tokio::test]
async fn acp_transport_with_client_tls_registers_via_wss() {
    let material = generate_material();
    let ca_path = write_pem(&material.dir, "ca.pem", &material.ca_pem);
    let client_cert_path = write_pem(&material.dir, "client.pem", &material.client_cert_pem);
    let client_key_path = write_pem(&material.dir, "client.key", &material.client_key_pem);

    let (addr, _server) = start_acp_mtls_server(&material).await;
    let url = format!("wss://localhost:{}/ws?server-key=test", addr.port());

    let tls = ClientTlsConfig::from_paths(&ca_path, &client_cert_path, &client_key_path);
    let transport = AcpTransport::new().with_client_tls(tls);

    let worker_id = WorkerId::new();
    transport
        .register(worker_id, &url, 1)
        .await
        .expect("register via mTLS should succeed");

    // 연결 상태 확인.
    assert!(
        transport.is_connected(worker_id).await,
        "worker should be Connected after mTLS register"
    );

    // 정리.
    transport.unregister(worker_id).await.expect("unregister");
}

/// mTLS 구성이 없는 경우 wss:// endpoint 는 register 가 실패해야 함
/// (공용 CA 를 신뢰하지만 서버 인증서가 사설 CA 로 서명되었으므로).
#[tokio::test]
async fn acp_transport_without_client_tls_fails_wss_to_private_ca() {
    let material = generate_material();
    let (addr, _server) = start_acp_mtls_server(&material).await;
    let url = format!("wss://localhost:{}/ws?server-key=test", addr.port());

    // client_tls 없이 기본 transport.
    let transport = AcpTransport::new();
    let worker_id = WorkerId::new();
    let result = transport.register(worker_id, &url, 1).await;
    assert!(
        result.is_err(),
        "register without mTLS config against private-CA server should fail, got: {result:?}"
    );
}
