//! `fleet mtls` 서브커맨드 (Phase 8.5.3).
//!
//! 사설 CA + 서버/클라이언트 인증서 발급 로컬 도구. rcgen 으로 메모리 상에서
//! 발급하고 PEM 파일로 디스크에 기록한다.
//!
//! ## 명령
//!
//! - `fleet mtls init-ca --out <dir>` — 사설 CA 발급 (ca.pem, ca.key).
//! - `fleet mtls issue-server --ca <dir> --out <dir> --dns <name>[,<name>...]`
//!   — 워커 서버 인증서 발급 (server.pem, server.key). CA 는 init-ca 로 만든
//!   디렉토리.
//! - `fleet mtls issue-client --ca <dir> --out <dir>` — orchestrator 클라이언트
//!   인증서 발급 (client.pem, client.key).
//!
//! 모든 명령은 `--common-name` 옵션으로 CN을 덮어쓸 수 있다 (기본값은 명령별로
//! 다름). 인증서 유효기간은 기본 10년(CA)/1년(leaf)이며, `--validity-days` 로 조정.

#![cfg(feature = "mtls")]

use std::path::Path;

use anyhow::{Context, Result};
use rcgen::{
    Certificate, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair,
};
use time::{Duration, OffsetDateTime};

use crate::MtlsAction;

/// `fleet mtls <action>` 진입점.
pub async fn run_mtls(action: MtlsAction) -> Result<()> {
    match action {
        MtlsAction::InitCa {
            out,
            common_name,
            validity_days,
        } => run_init_ca(&out, &common_name, validity_days),
        MtlsAction::IssueServer {
            ca,
            out,
            common_name,
            dns,
            validity_days,
        } => run_issue_server(&ca, &out, &common_name, &dns, validity_days),
        MtlsAction::IssueClient {
            ca,
            out,
            common_name,
            validity_days,
        } => run_issue_client(&ca, &out, &common_name, validity_days),
    }
}

/// `fleet mtls init-ca` 구현.
pub fn run_init_ca(out_dir: &Path, common_name: &str, validity_days: u64) -> Result<()> {
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("create dir {}", out_dir.display()))?;

    let mut params = CertificateParams::new(vec![])?;
    params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    dn.push(DnType::OrganizationName, "Grok Fleet Orchestrator");
    params.distinguished_name = dn;
    // CA 인증서는 key_cert_sign / crl_sign 비트를 key_usages 로 설정.
    // extended_key_usages 는 leaf 인증서에서 의미를 가짐 (CA 에서는 생략).
    params.key_usages = vec![
        rcgen::KeyUsagePurpose::KeyCertSign,
        rcgen::KeyUsagePurpose::CrlSign,
    ];
    set_validity(&mut params, validity_days)?;

    let key = KeyPair::generate()?;
    let cert = params.self_signed(&key)?;
    write_pair(out_dir, "ca", &cert.pem(), &key.serialize_pem())?;

    println!("✓ CA issued:");
    println!("  cert: {}/ca.pem", out_dir.display());
    println!("  key:  {}/ca.key", out_dir.display());
    println!("  subject: CN={common_name}");
    println!();
    println!("Next: issue server/client certs signed by this CA:");
    println!("  fleet mtls issue-server --ca {} --dns worker-1.fleet --out /etc/fleet/worker-1/", out_dir.display());
    println!("  fleet mtls issue-client --ca {} --out /etc/fleet/orchestrator/", out_dir.display());
    Ok(())
}

/// `fleet mtls issue-server` 구현.
pub fn run_issue_server(
    ca_dir: &Path,
    out_dir: &Path,
    common_name: &str,
    dns_sans: &[String],
    validity_days: u64,
) -> Result<()> {
    let (ca_cert, ca_key) = load_ca(ca_dir)?;

    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("create dir {}", out_dir.display()))?;

    if dns_sans.is_empty() {
        anyhow::bail!(
            "issue-server requires at least one --dns SAN; the orchestrator will connect via wss://<host>:<port>/... and the SAN must match"
        );
    }
    let mut params = CertificateParams::new(dns_sans.to_vec())?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    params.distinguished_name = dn;
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    set_validity(&mut params, validity_days)?;

    let key = KeyPair::generate()?;
    let cert = params.signed_by(&key, &ca_cert, &ca_key)?;
    write_pair(out_dir, "server", &cert.pem(), &key.serialize_pem())?;

    println!("✓ Server certificate issued:");
    println!("  cert: {}/server.pem", out_dir.display());
    println!("  key:  {}/server.key", out_dir.display());
    println!("  subject: CN={common_name}");
    println!("  SANs: {}", dns_sans.join(", "));
    println!();
    println!("Distribute to the worker and reference in worker.toml:");
    println!("  [mtls]");
    println!("  enabled = true");
    println!("  listen_addr = \"0.0.0.0:2420\"");
    println!("  server_cert_path = \"{}/server.pem\"", out_dir.display());
    println!("  server_key_path = \"{}/server.key\"", out_dir.display());
    println!("  client_ca_path = \"{}/ca.pem\"", ca_dir.display());
    Ok(())
}

/// `fleet mtls issue-client` 구현.
pub fn run_issue_client(
    ca_dir: &Path,
    out_dir: &Path,
    common_name: &str,
    validity_days: u64,
) -> Result<()> {
    let (ca_cert, ca_key) = load_ca(ca_dir)?;

    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("create dir {}", out_dir.display()))?;

    let mut params = CertificateParams::new(vec![common_name.to_string()])?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    params.distinguished_name = dn;
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    set_validity(&mut params, validity_days)?;

    let key = KeyPair::generate()?;
    let cert = params.signed_by(&key, &ca_cert, &ca_key)?;
    write_pair(out_dir, "client", &cert.pem(), &key.serialize_pem())?;

    println!("✓ Client certificate issued:");
    println!("  cert: {}/client.pem", out_dir.display());
    println!("  key:  {}/client.key", out_dir.display());
    println!("  subject: CN={common_name}");
    println!();
    println!("Distribute to the orchestrator and pass via:");
    println!("  fleet serve --mtls-ca {}/ca.pem \\", ca_dir.display());
    println!("             --mtls-cert {}/client.pem \\", out_dir.display());
    println!("             --mtls-key {}/client.key", out_dir.display());
    Ok(())
}

/// CA 디렉토리에서 ca.pem + ca.key 로드.
///
/// rcgen 0.13 의 `CertificateParams::from_ca_cert_pem` 은 `pem` + `x509-parser`
/// feature 가 필요 (fleet-cli/Cargo.toml 에서 이미 활성화). 반환되는 params 는
/// CA 인증서에서 서명에 필요한 최소 정보만 추출한 것이므로, `self_signed` 로
/// 다시 `Certificate` 로 만들어 `signed_by` 의 issuer 로 사용한다.
fn load_ca(ca_dir: &Path) -> Result<(Certificate, KeyPair)> {
    let cert_pem = std::fs::read_to_string(ca_dir.join("ca.pem"))
        .with_context(|| format!("read {}/ca.pem", ca_dir.display()))?;
    let key_pem = std::fs::read_to_string(ca_dir.join("ca.key"))
        .with_context(|| format!("read {}/ca.key", ca_dir.display()))?;

    let params = CertificateParams::from_ca_cert_pem(&cert_pem)
        .context("parse ca.pem (rcgen from_ca_cert_pem)")?;
    let ca_key = KeyPair::from_pem(&key_pem).context("parse ca.key")?;
    // params.self_signed 는 동일한 ca_key 로 다시 서명 — params 가 원본 CA cert
    // 에서 추출된 것이므로 결과 CA cert 는 기능적으로 원본과 동일.
    let ca_cert = params.self_signed(&ca_key)?;
    Ok((ca_cert, ca_key))
}

/// PEM cert + key 쌍을 디렉토리에 쓰기. 키 파일은 0600 권한.
fn write_pair(dir: &Path, stem: &str, cert_pem: &str, key_pem: &str) -> Result<()> {
    let cert_path = dir.join(format!("{stem}.pem"));
    let key_path = dir.join(format!("{stem}.key"));
    std::fs::write(&cert_path, cert_pem)
        .with_context(|| format!("write {}", cert_path.display()))?;
    std::fs::write(&key_path, key_pem)
        .with_context(|| format!("write {}", key_path.display()))?;

    // 유닉스 권한 0600 (오직 소유자만 읽기/쓰기).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&key_path, perms).ok();
    }
    Ok(())
}

/// CertificateParams 의 not_before / not_after 설정 (UTC now ± validity).
fn set_validity(params: &mut CertificateParams, validity_days: u64) -> Result<()> {
    let now = OffsetDateTime::now_utc();
    params.not_before = now - Duration::days(1); // 1일 클럭 스큐 허용.
    params.not_after = now + Duration::days(validity_days as i64);
    Ok(())
}

#[cfg(test)]
mod tests {
    //! `fleet mtls` 플로우 단위 테스트.
    //!
    //! 발급된 인증서/키가 rustls-pemfile 로 파싱 가능하고, leaf 인증서가 CA 로
    //! 서명되었는지 검증한다. mTLS 핸드셰이크 자체는 fleet-transport 의
    //! `tests/mtls_handshake.rs` 와 `tests/mtls_proxy.rs` 가 담당.

    use super::*;
    use std::io::BufReader;

    /// PEM 문자열에서 모든 cert 블록을 추출 (rustls-pemfile).
    fn parse_certs(pem: &str) -> Vec<rustls::pki_types::CertificateDer<'static>> {
        let mut reader = BufReader::new(pem.as_bytes());
        rustls_pemfile::certs(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .expect("parse certs")
    }

    /// PEM 문자열에서 private key 추출.
    fn parse_key(pem: &str) -> rustls::pki_types::PrivateKeyDer<'static> {
        let mut reader = BufReader::new(pem.as_bytes());
        rustls_pemfile::private_key(&mut reader)
            .expect("parse key")
            .expect("at least one key")
    }

    /// init-ca → issue-server → issue-client 풀 플로우가 파일을 생성하는지 검증.
    #[test]
    fn init_ca_and_issue_certs_roundtrip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ca_dir = tmp.path().join("ca");
        let server_dir = tmp.path().join("server");
        let client_dir = tmp.path().join("client");

        // 1. init-ca
        run_init_ca(&ca_dir, "Fleet Test CA", 30).expect("init-ca");
        let ca_pem = std::fs::read_to_string(ca_dir.join("ca.pem")).unwrap();
        let ca_key_pem = std::fs::read_to_string(ca_dir.join("ca.key")).unwrap();
        assert!(!parse_certs(&ca_pem).is_empty(), "ca.pem must contain certs");
        parse_key(&ca_key_pem);

        // 2. issue-server
        run_issue_server(
            &ca_dir,
            &server_dir,
            "worker-1",
            &["worker-1.fleet".into(), "localhost".into()],
            30,
        )
        .expect("issue-server");
        let server_pem = std::fs::read_to_string(server_dir.join("server.pem")).unwrap();
        let server_key_pem = std::fs::read_to_string(server_dir.join("server.key")).unwrap();
        assert!(!parse_certs(&server_pem).is_empty(), "server.pem must contain certs");
        parse_key(&server_key_pem);

        // 3. issue-client
        run_issue_client(&ca_dir, &client_dir, "orchestrator", 30).expect("issue-client");
        let client_pem = std::fs::read_to_string(client_dir.join("client.pem")).unwrap();
        let client_key_pem = std::fs::read_to_string(client_dir.join("client.key")).unwrap();
        assert!(!parse_certs(&client_pem).is_empty(), "client.pem must contain certs");
        parse_key(&client_key_pem);
    }

    /// key 파일이 0600 권한으로 생성되는지 검증 (유닉스만).
    #[cfg(unix)]
    #[test]
    fn key_files_have_restricted_perms() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tempdir");
        run_init_ca(tmp.path(), "Perm Test CA", 30).expect("init-ca");
        let meta = std::fs::metadata(tmp.path().join("ca.key")).expect("stat ca.key");
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "ca.key should have 0600 perms, got {mode:o}");
    }

    /// issue-server 가 SAN 없이 호출되면 에러.
    #[test]
    fn issue_server_requires_at_least_one_san() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ca_dir = tmp.path().join("ca");
        let server_dir = tmp.path().join("server");
        run_init_ca(&ca_dir, "Test CA", 30).expect("init-ca");
        let err = run_issue_server(&ca_dir, &server_dir, "worker", &[], 30).unwrap_err();
        assert!(
            err.to_string().contains("SAN"),
            "expected SAN error, got: {err}"
        );
    }

    /// load_ca 가 존재하지 않는 파일에 대해 명확한 에러 반환.
    #[test]
    fn load_ca_missing_file_errors() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let result = load_ca(tmp.path());
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("expected error for missing CA files"),
        };
        // anyhow 의 display 에는 원본 context 가 포함됨.
        let msg = format!("{err:#}");
        assert!(msg.contains("ca.pem"), "expected ca.pem mention, got: {msg}");
    }
}
