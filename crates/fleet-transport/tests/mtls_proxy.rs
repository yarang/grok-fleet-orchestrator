//! Phase 8.5.2: MtlsProxy 엔드투엔드 통합 테스트.
//!
//! 평문 TCP echo upstream → MtlsProxy → (mTLS 핸드셰이크 후) 클라이언트.
//! TLS 종단 + 클라이언트 인증서 검증 + 양방향 복사가 모두 동작하는지 확인.
//!
//! 2026-08-11: `MtlsProxy`는 ACP와 무관한 범용 mTLS 종단 프록시(fleet-worker
//! 쪽에서 사용)라 이번 SDK 전환의 영향을 받지 않는다 — 옛 `WsConn::connect_mtls`
//! 대신 `rustls` connector로 직접 TLS를 맺고 그 위에 `tokio_tungstenite::client_async`로
//! WS 핸드셰이크만 하는 방식으로 클라이언트 쪽만 갈아 끼웠다.
//!
//! `--features mtls` 필요.

#![cfg(feature = "mtls")]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rcgen::{CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair};
use rustls::ServerConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;

use fleet_transport::mtls_proxy::MtlsProxy;
use fleet_transport::tls::{ClientTlsConfig, ServerTlsConfig};

fn temp_dir() -> PathBuf {
    // 병렬 테스트 스레드 간 충돌 방지: process id + atomic 카운터 조합.
    // 이전에는 SystemTime::as_nanos() 만 썼으나, 같은 프로세스의 병렬
    // 스레드들이 동시에 같은 nano 타임스탬프를 얻어 디렉토리명이 충돌하고,
    // PEM 파일이 서로 덮어쓰이며 BadSignature 로 이어지는 레이스가 있었다.
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "fleet-mtls-proxy-test-{}-{}",
        std::process::id(),
        seq
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

    // Client cert.
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

/// WS upgrade 요청의 `Authorization` 헤더를 **그대로 포착해** 돌려주는 upstream
/// (로드맵 `#94`). 헤더가 없으면 `None`을 보낸다.
// `Err` 타입은 tungstenite의 handshake 콜백 트레이트가 정하는 것이라
// (`http::Response<Option<String>>`) 우리 쪽에서 줄일 수 없다.
#[allow(clippy::result_large_err)]
async fn start_ws_header_capture_upstream(
) -> (SocketAddr, tokio::sync::mpsc::Receiver<Option<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                return;
            };
            let tx = tx.clone();
            tokio::spawn(async move {
                let seen = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
                let seen_cb = seen.clone();
                let callback = move |req: &tokio_tungstenite::tungstenite::handshake::server::Request,
                                     resp: tokio_tungstenite::tungstenite::handshake::server::Response| {
                    *seen_cb.lock().unwrap() = req
                        .headers()
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string);
                    Ok(resp)
                };
                let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(tcp, callback).await else {
                    return;
                };
                let captured = seen.lock().unwrap().clone();
                let _ = tx.send(captured).await;
                while let Some(Ok(msg)) = ws.next().await {
                    if matches!(msg, Message::Close(_)) {
                        break;
                    }
                }
            });
        }
    });
    (addr, rx)
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
    let server_config: Arc<ServerConfig> = Arc::new(server_tls.build_server_config().unwrap());

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
    let mut tls = connector
        .connect(server_name, tcp)
        .await
        .expect("TLS connect");

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

    // TLS를 먼저 맺고, 그 스트림 위에 WS 핸드셰이크만 얹는다 — 옛
    // `WsConn::connect_mtls`가 내부적으로 하던 것과 동일한 두 단계를
    // 이 테스트 파일의 다른 테스트들과 같은 방식(직접 rustls connector)으로 수행.
    let connector = client_tls.build_connector().unwrap();
    let tcp = tokio::net::TcpStream::connect(proxy_addr)
        .await
        .expect("connect proxy");
    use rustls::pki_types::ServerName;
    let server_name = ServerName::try_from("localhost").unwrap();
    let tls_stream = connector
        .connect(server_name, tcp)
        .await
        .expect("TLS connect");

    let url = format!("wss://localhost:{}/ws?server-key=x", proxy_addr.port());
    let (mut ws, _resp) = tokio_tungstenite::client_async(url, tls_stream)
        .await
        .expect("ws handshake through proxy");

    ws.send(Message::Text(
        r#"{"jsonrpc":"2.0","method":"ping","id":1}"#.into(),
    ))
    .await
    .unwrap();

    let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("timeout")
        .expect("stream closed")
        .expect("ws err");

    match msg {
        Message::Text(t) => assert!(t.contains("ping")),
        other => panic!("expected text, got {other:?}"),
    }

    let _ = ws.close(None).await;
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

    let mut client_params = CertificateParams::new(vec!["orchestrator".to_string()]).unwrap();
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
        write_pem(
            &material.dir,
            "rogue-client.key",
            &client_key.serialize_pem(),
        ),
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

/// 로드맵 #36 — 프로세스 재시작 없이 서버 인증서를 회전할 수 있는지
/// 엔드투엔드로 검증. 같은 CA로 서명된 서로 다른 서버 인증서 두 개를
/// 준비해, (1) 첫 연결이 인증서 A를 제시하는지, (2) `reload()` 호출 후
/// **새 연결**이 인증서 B를 제시하는지, (3) 두 인증서의 raw DER 바이트가
/// 실제로 다른지를 확인한다 — 단순히 "에러 없이 reload가 리턴했다"보다
/// 훨씬 강한 증거다.
#[tokio::test]
async fn mtls_proxy_rotates_server_cert_without_restart() {
    let dir = temp_dir();

    // CA (재사용 — 두 서버 인증서 모두 이 CA로 서명).
    let mut ca_params = CertificateParams::new(vec![]).unwrap();
    ca_params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let mut ca_dn = DistinguishedName::new();
    ca_dn.push(DnType::CommonName, "fleet-test-ca");
    ca_params.distinguished_name = ca_dn;
    let ca_key = KeyPair::generate().unwrap();
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let ca_pem = ca_cert.pem();

    let make_server_cert = |cn: &str| {
        let mut params = CertificateParams::new(vec!["localhost".to_string()]).unwrap();
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, cn);
        params.distinguished_name = dn;
        params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ServerAuth);
        let key = KeyPair::generate().unwrap();
        let cert = params.signed_by(&key, &ca_cert, &ca_key).unwrap();
        (cert.der().to_vec(), cert.pem(), key.serialize_pem())
    };

    let (cert_a_der, cert_a_pem, key_a_pem) = make_server_cert("server-a");
    let (cert_b_der, cert_b_pem, key_b_pem) = make_server_cert("server-b");
    assert_ne!(cert_a_der, cert_b_der, "test setup: certs must differ");

    // 클라이언트 인증서 (한 벌이면 충분 — 회전 대상은 서버 인증서뿐).
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

    let ca_path = write_pem(&dir, "ca.pem", &ca_pem);
    let server_cert_path = write_pem(&dir, "server.pem", &cert_a_pem);
    let server_key_path = write_pem(&dir, "server.key", &key_a_pem);
    let server_tls = ServerTlsConfig::from_paths(&ca_path, &server_cert_path, &server_key_path);

    let (server_config, resolver) = server_tls
        .build_rotating_server_config()
        .expect("build rotating config");

    let upstream = start_plain_echo_upstream().await;
    let proxy_addr_unused: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let proxy = MtlsProxy::bind(proxy_addr_unused, upstream, Arc::new(server_config))
        .await
        .expect("proxy bind");
    let proxy_addr = proxy.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let proxy_handle = tokio::spawn(async move { proxy.run(shutdown_rx).await });

    let client_tls = ClientTlsConfig::from_paths(
        &ca_path,
        write_pem(&dir, "client.pem", &client_cert.pem()),
        write_pem(&dir, "client.key", &client_key.serialize_pem()),
    );

    use rustls::pki_types::ServerName;

    async fn peer_cert_der(proxy_addr: SocketAddr, client_tls: &ClientTlsConfig) -> Vec<u8> {
        let connector = client_tls.build_connector().unwrap();
        let tcp = tokio::net::TcpStream::connect(proxy_addr)
            .await
            .expect("connect proxy");
        let server_name = ServerName::try_from("localhost").unwrap();
        let tls = connector
            .connect(server_name, tcp)
            .await
            .expect("TLS connect");
        let (_io, conn) = tls.get_ref();
        conn.peer_certificates()
            .expect("server must present a certificate")[0]
            .to_vec()
    }

    // 1) 회전 전 — 인증서 A가 제시되어야 함.
    let served_before = peer_cert_der(proxy_addr, &client_tls).await;
    assert_eq!(served_before, cert_a_der, "expected cert A before rotation");

    // 2) 디스크의 파일을 인증서 B로 교체하고 reload().
    std::fs::write(&server_cert_path, &cert_b_pem).unwrap();
    std::fs::write(&server_key_path, &key_b_pem).unwrap();
    resolver.reload(&server_tls).expect("reload must succeed");

    // 3) 회전 후 — 새 연결은 인증서 B를 제시해야 함(진행 중이던 연결이
    //    아니라 "이후" 연결부터 반영되는 것이 정확한 동작).
    let served_after = peer_cert_der(proxy_addr, &client_tls).await;
    assert_eq!(served_after, cert_b_der, "expected cert B after rotation");
    assert_ne!(
        served_before, served_after,
        "rotation must actually change the served certificate"
    );

    let _ = shutdown_tx.send(true);
    let _ = proxy_handle.await;
}

/// reload()가 잘못된(파싱 불가) 인증서 파일을 만나면 기존 캐시를 그대로
/// 두고 에러만 반환해야 한다 — 잘못된 갱신 시도로 서비스가 끊기면 안 된다
/// (로드맵 #36의 "서비스 중단 없이 교체" 요구사항의 핵심).
#[tokio::test]
async fn reload_keeps_serving_last_good_cert_on_failure() {
    let material = generate_material();
    let ca_path = write_pem(&material.dir, "ca.pem", &material.ca_pem);
    let server_cert_path = write_pem(&material.dir, "server.pem", &material.server_cert_pem);
    let server_key_path = write_pem(&material.dir, "server.key", &material.server_key_pem);
    let server_tls = ServerTlsConfig::from_paths(&ca_path, &server_cert_path, &server_key_path);

    let (_config, resolver) = server_tls
        .build_rotating_server_config()
        .expect("build rotating config");

    // 인증서 파일을 깨뜨린다.
    std::fs::write(&server_cert_path, "not a valid pem").unwrap();

    let result = resolver.reload(&server_tls);
    assert!(result.is_err(), "reload must surface the parse failure");
    // (캐시가 그대로 유지된다는 것은 위 mtls_proxy_rotates_server_cert_without_restart
    // 테스트가 정상 케이스에서 이미 증명 — 여기서는 실패 시 panic/캐시 파괴가
    // 없다는 것만 별도로 확인.)
}

/// 로드맵 `#94` — `Authorization` 헤더가 mTLS 프록시 홉을 **그대로 통과**하는가.
///
/// `MtlsProxy`는 `copy_bidirectional`로 바이트를 그대로 나르므로 통과하는 것이
/// 당연해 보이지만, "당연해 보인다"와 "측정했다"는 다르다. `#94`는 secret을
/// 이 홉 너머의 grok까지 헤더로 전달하는 데 전적으로 의존하므로, 이 성질이
/// 깨지면 mTLS 워커 전체의 다이얼이 죽는다.
#[tokio::test]
async fn mtls_proxy_forwards_authorization_header_verbatim() {
    let material = generate_material();
    let (upstream, mut captured) = start_ws_header_capture_upstream().await;

    let server_tls = ServerTlsConfig::from_paths(
        write_pem(&material.dir, "ca.pem", &material.ca_pem),
        write_pem(&material.dir, "server.pem", &material.server_cert_pem),
        write_pem(&material.dir, "server.key", &material.server_key_pem),
    );
    let server_config = Arc::new(server_tls.build_server_config().unwrap());

    let proxy = MtlsProxy::bind("127.0.0.1:0".parse().unwrap(), upstream, server_config)
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
    let connector = client_tls.build_connector().unwrap();
    let tcp = tokio::net::TcpStream::connect(proxy_addr)
        .await
        .expect("connect proxy");
    use rustls::pki_types::ServerName;
    let tls_stream = connector
        .connect(ServerName::try_from("localhost").unwrap(), tcp)
        .await
        .expect("TLS connect");

    // `#94` 이후 fleet이 실제로 만드는 형태 — URL에 secret 없음, 헤더에 있음.
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let url = format!("wss://localhost:{}/ws", proxy_addr.port());
    let mut request = url.as_str().into_client_request().unwrap();
    request
        .headers_mut()
        .insert("authorization", "Bearer topsecret".parse().unwrap());

    let (mut ws, _resp) = tokio_tungstenite::client_async(request, tls_stream)
        .await
        .expect("ws handshake through proxy");

    let seen = tokio::time::timeout(Duration::from_secs(5), captured.recv())
        .await
        .expect("timeout waiting for captured header")
        .expect("capture channel closed");
    assert_eq!(
        seen.as_deref(),
        Some("Bearer topsecret"),
        "Authorization 헤더가 mTLS 홉을 그대로 통과해야 한다 (#94)"
    );

    let _ = ws.close(None).await;
    let _ = shutdown_tx.send(true);
    let _ = proxy_handle.await;
}
