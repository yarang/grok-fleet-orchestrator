//! mTLS 클라이언트 구성 (Phase 8.5).
//!
//! 이 모듈은 orchestrator가 worker의 mTLS 종단 proxy에 연결할 때 사용하는
//! rustls 기반 TLS 클라이언트 구성을 제공합니다. 공용 CA (webpki-roots)를
//! 신뢰하지 않고 오직 사설 CA만 신뢰하며, 클라이언트 인증서로 자신을 증명합니다.
//!
//! ## 사용 흐름
//!
//! 1. 어드민이 `fleet mtls init-ca` + `fleet mtls issue-client`로
//!    사설 CA + orchestrator 클라이언트 인증서를 발급.
//! 2. orchestrator 구동 시 `--mtls-ca <ca.pem> --mtls-cert <client.pem>
//!    --mtls-key <client.key>` 플래그 전달.
//! 3. `ClientTlsConfig::from_paths` 로 세 파일을 읽어 `ClientTlsConfig` 생성.
//! 4. `AcpTransport` 또는 `WsConn` 이 이 구성을 사용해 `wss://` 엔드포인트에
//!    mTLS 핸드셰이크를 수행.
//!
//! ## 검증 모델
//!
//! - 신뢰: 오직 `ca_cert_path` 의 PEM만 `RootCertStore`에 추가.
//! - 서버 이름: 연결 시 URL에서 추출한 host:port를 `ServerName`으로 전달.
//!   worker의 서버 인증서 SAN이 이 host와 일치해야 함.
//! - 클라이언트 인증: `client_cert_path` + `client_key_path` 로 체인 전체를
//!   전송 (leaf + intermediate).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, RootCertStore};
use thiserror::Error;
use tokio_rustls::TlsConnector;

/// mTLS 클라이언트 구성.
///
/// 세 PEM 파일 경로를 보관하며, `build_connector()` 로 매번 새 connector를
/// 생성할 수 있다. 파일은 핸드셰이크 직전에 다시 읽으므로 인증서 갱신 시
/// 프로세스 재시작 없이 자동 반영된다.
#[derive(Debug, Clone)]
pub struct ClientTlsConfig {
    /// 신뢰하는 사설 CA 인증서 (PEM).
    pub ca_cert_path: PathBuf,
    /// orchestrator 클라이언트 인증서 (PEM).
    pub client_cert_path: PathBuf,
    /// orchestrator 클라이언트 비밀키 (PEM, PKCS#8 권장).
    pub client_key_path: PathBuf,
}

/// TLS 구성/파싱 에러.
#[derive(Debug, Error)]
pub enum TlsError {
    #[error("failed to read {what} from {path}: {source}")]
    ReadFile {
        what: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("no client private key found in {0}")]
    NoClientKey(PathBuf),
    #[error("failed to build rustls ClientConfig: {0}")]
    Build(String),
}

impl ClientTlsConfig {
    /// 세 PEM 파일 경로로 구성 생성. 파일 존재 여부는 `build_connector` 시점에 검증.
    pub fn from_paths(
        ca: impl Into<PathBuf>,
        client_cert: impl Into<PathBuf>,
        client_key: impl Into<PathBuf>,
    ) -> Self {
        Self {
            ca_cert_path: ca.into(),
            client_cert_path: client_cert.into(),
            client_key_path: client_key.into(),
        }
    }

    /// 파일에서 rustls `TlsConnector` 빌드.
    ///
    /// 매 호출마다 파일을 다시 읽는다 (오버헤드 수 밀리초). 캐싱은 호출자 책임.
    pub fn build_connector(&self) -> Result<TlsConnector, TlsError> {
        let client_config = self.build_client_config()?;
        Ok(TlsConnector::from(Arc::new(client_config)))
    }

    /// rustls `ClientConfig` 빌드 (테스트/재사용용 노출).
    pub fn build_client_config(&self) -> Result<ClientConfig, TlsError> {
        let ca_certs = load_certs(&self.ca_cert_path, "CA")?;
        let client_chain = load_certs(&self.client_cert_path, "client cert")?;
        let client_key = load_private_key(&self.client_key_path)?;

        if ca_certs.is_empty() {
            return Err(TlsError::Build(format!(
                "no CA certificates in {}",
                self.ca_cert_path.display()
            )));
        }
        if client_chain.is_empty() {
            return Err(TlsError::Build(format!(
                "no client certificate in {}",
                self.client_cert_path.display()
            )));
        }

        let mut root_store = RootCertStore::empty();
        for cert in &ca_certs {
            root_store
                .add(cert.clone())
                .map_err(|e| TlsError::Build(format!("add CA root: {e}")))?;
        }

        // rustls 0.23 은 프로세스 단위 CryptoProvider가 필요. aws_lc_rs 가 transitively
        // 포함되는 경우가 많으므로, 명시적으로 ring provider를 사용.
        let provider = rustls::crypto::ring::default_provider();
        ClientConfig::builder_with_provider(Arc::new(provider))
            .with_safe_default_protocol_versions()
            .map_err(|e| TlsError::Build(format!("protocol versions: {e}")))?
            .with_root_certificates(root_store)
            .with_client_auth_cert(client_chain, client_key)
            .map_err(|e| TlsError::Build(format!("with_client_auth_cert: {e}")))
    }
}

/// 서버 측 mTLS 구성 (worker 의 종단 proxy용).
///
/// `ca_path` 에 지정된 PEM 으로 서명된 클라이언트 인증서만 핸드셰이크를 통과.
/// 자신의 서버 인증서는 `server_cert_path` + `server_key_path` 로 제출.
#[derive(Debug, Clone)]
pub struct ServerTlsConfig {
    /// 클라이언트 인증서 검증용 CA (PEM).
    pub ca_path: PathBuf,
    /// 서버 인증서 체인 (PEM).
    pub server_cert_path: PathBuf,
    /// 서버 비밀키 (PEM).
    pub server_key_path: PathBuf,
}

impl ServerTlsConfig {
    pub fn from_paths(
        ca: impl Into<PathBuf>,
        server_cert: impl Into<PathBuf>,
        server_key: impl Into<PathBuf>,
    ) -> Self {
        Self {
            ca_path: ca.into(),
            server_cert_path: server_cert.into(),
            server_key_path: server_key.into(),
        }
    }

    /// rustls `ServerConfig` 빌드. 클라이언트 인증서 강제 (mTLS).
    pub fn build_server_config(&self) -> Result<rustls::ServerConfig, TlsError> {
        let ca_certs = load_certs(&self.ca_path, "CA")?;
        let server_chain = load_certs(&self.server_cert_path, "server cert")?;
        let server_key = load_private_key(&self.server_key_path)?;

        if ca_certs.is_empty() {
            return Err(TlsError::Build(format!(
                "no CA certificates in {}",
                self.ca_path.display()
            )));
        }
        if server_chain.is_empty() {
            return Err(TlsError::Build(format!(
                "no server certificate in {}",
                self.server_cert_path.display()
            )));
        }

        let mut client_roots = RootCertStore::empty();
        for cert in &ca_certs {
            client_roots
                .add(cert.clone())
                .map_err(|e| TlsError::Build(format!("add client CA root: {e}")))?;
        }

        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            Arc::new(client_roots),
            provider.clone(),
        )
        .build()
        .map_err(|e| TlsError::Build(format!("client verifier: {e}")))?;

        rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| TlsError::Build(format!("protocol versions: {e}")))?
            .with_client_cert_verifier(verifier)
            .with_single_cert(server_chain, server_key)
            .map_err(|e| TlsError::Build(format!("with_single_cert: {e}")))
    }
}

/// PEM 파일에서 인증서 체인 로드.
pub(crate) fn load_certs(
    path: &Path,
    what: &'static str,
) -> Result<Vec<CertificateDer<'static>>, TlsError> {
    let bytes = std::fs::read(path).map_err(|source| TlsError::ReadFile {
        what,
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = std::io::BufReader::new(&bytes[..]);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TlsError::Build(format!("parse PEM {what}: {e}")))?;
    Ok(certs)
}

/// PEM 파일에서 개인키 로드 (PKCS#8 / PKCS#1 / SEC1 자동 감지).
pub(crate) fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, TlsError> {
    let bytes = std::fs::read(path).map_err(|source| TlsError::ReadFile {
        what: "client key",
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = std::io::BufReader::new(&bytes[..]);
    let key = rustls_pemfile::private_key(&mut reader)
        .map_err(|e| TlsError::Build(format!("parse PEM private key: {e}")))?
        .ok_or_else(|| TlsError::NoClientKey(path.to_path_buf()))?;
    Ok(key)
}
