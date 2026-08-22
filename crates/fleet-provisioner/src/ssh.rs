//! SSH 클라이언트 — `RemoteExecutor` 트레이트와 두 구현체.
//!
//! - `SshClient`: `russh` 기반 실제 SSH 연결 (`ssh` feature 필요).
//! - `MockExecutor`: 테스트용 인메모리 구현. 사전 프로그래밍된 응답 반환.
//!
//! ## 설계 의도
//!
//! 모든 프로비저닝 스텝은 `&dyn RemoteExecutor`를 받습니다. 프로덕션에서는
//! `SshClient`를, 테스트에서는 `MockExecutor`를 주입합니다. 이로써 스텝 로직을
//! 실제 SSH 서버 없이 100% 검증할 수 있습니다.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::error::SshError;

/// 원격 명령 실행기 추상화.
///
/// `SshClient`와 `MockExecutor`가 구현합니다. `async fn`을 트레이트로
/// 노출하기 위해 `async-trait` 사용.
#[async_trait]
pub trait RemoteExecutor: Send + Sync {
    /// 동기적으로 명령 실행, stdout/stderr를 합쳐서 반환.
    /// 종료 코드가 0이 아닌 경우 `SshError::Protocol` 반환하지 않고
    /// 호출자가 직접 처리할 수 있도록 stdout 문자열 그대로 반환.
    async fn exec(&self, command: &str) -> Result<String, SshError>;

    /// 스트리밍 실행 — 각 출력 라인을 콜백으로 전달. 종료 코드 반환.
    /// `Box<dyn FnMut>`를 사용해 trait이 dyn-compatible하도록 함.
    async fn exec_streaming(
        &self,
        command: &str,
        on_line: Box<dyn for<'a> FnMut(&'a str) + Send>,
    ) -> Result<i32, SshError>;

    /// 로컬 파일을 원격으로 업로드. `mode`는 8진수 (예: `0o755`).
    async fn upload_file(
        &self,
        local_path: &str,
        remote_path: &str,
        mode: u32,
    ) -> Result<(), SshError>;

    /// 원격 경로에 content를 직접 작성.
    async fn write_file(&self, path: &str, content: &str) -> Result<(), SshError>;
}

/// SSH 접속 정보. 재연결이나 진단 로그에 활용.
#[derive(Debug, Clone)]
pub struct SshConnectInfo {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub key_path: PathBuf,
}

impl SshConnectInfo {
    pub fn new(host: impl Into<String>, user: impl Into<String>, key_path: PathBuf) -> Self {
        Self {
            host: host.into(),
            port: 22,
            user: user.into(),
            key_path,
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }
}

/// 서버 호스트 키 검증 정책.
///
/// SSH 연결 시 원격 서버가 제시한 공개키를 어떻게 검증할지 결정.
/// MITM(중간자 공격) 방어의 핵심 — OpenSSH의 `StrictHostKeyChecking` 설정과 대응.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyPolicy {
    /// 서버 키를 검증 없이 무조건 수용. **보안상 위험** — MITM에 취약.
    /// 개발/테스트 환경이나 일회성 연결에서만 사용.
    /// OpenSSH `StrictHostKeyChecking=no` 에 대응.
    AcceptAll,
    /// Trust On First use. 첫 연결에서 서버 키를 `known_hosts`에 자동 추가,
    /// 이후 연결에서는 저장된 키와 일치해야 통과. OpenSSH 기본 동작에 대응.
    /// (`StrictHostKeyChecking=accept-new`)
    Tofu,
    /// Strict. `known_hosts`에 호스트가 **반드시** 있어야 하고 키가 일치해야 함.
    /// 자동 추가 없음. 알려진 인프라에만 연결하는 운영 환경 권장.
    /// OpenSSH `StrictHostKeyChecking=yes` 에 대응.
    Strict,
}

impl HostKeyPolicy {
    /// CLI/인벤토리에서 받은 문자열 값을 정책으로 파싱.
    /// 대소문자 무관, `-`/`_`/공백 구분 무시.
    pub fn parse(s: &str) -> Result<Self, String> {
        let norm = s.to_ascii_lowercase();
        let norm = norm.replace(['_', ' ', '-'], "");
        match norm.as_str() {
            "acceptall" | "no" | "insecure" => Ok(Self::AcceptAll),
            "tofu" | "acceptnew" | "auto" => Ok(Self::Tofu),
            "strict" | "yes" => Ok(Self::Strict),
            _ => Err(format!(
                "unknown host key policy '{s}' (expected: accept-all | tofu | strict)"
            )),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AcceptAll => "accept-all",
            Self::Tofu => "tofu",
            Self::Strict => "strict",
        }
    }
}

impl Default for HostKeyPolicy {
    /// 기본값은 TOFU — OpenSSH 표준 동작.
    /// AcceptAll 은 보안상 위험하므로 명시적 opt-in으로만 사용.
    fn default() -> Self {
        Self::Tofu
    }
}

impl std::fmt::Display for HostKeyPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 호스트 키 검증 구성. 정책 + known_hosts 파일 경로.
#[derive(Debug, Clone)]
pub struct HostKeyConfig {
    /// 검증 정책.
    pub policy: HostKeyPolicy,
    /// `known_hosts` 파일 경로. `None` 이면 기본값(`~/.ssh/known_hosts`) 사용.
    /// `AcceptAll` 정책에서는 무시됨.
    pub known_hosts_path: Option<PathBuf>,
}

impl HostKeyConfig {
    /// 지정된 정책과 기본 known_hosts 경로로 구성.
    pub fn new(policy: HostKeyPolicy) -> Self {
        Self {
            policy,
            known_hosts_path: None,
        }
    }

    /// known_hosts 파일 경로 오버라이드.
    pub fn with_known_hosts(mut self, path: impl Into<PathBuf>) -> Self {
        self.known_hosts_path = Some(path.into());
        self
    }

    /// 실제 사용할 known_hosts 경로. 명시값 우선, 없으면 `~/.ssh/known_hosts`.
    /// `HOME` 환경변수가 없으면 `None`.
    pub fn effective_known_hosts(&self) -> Option<PathBuf> {
        self.known_hosts_path
            .clone()
            .or_else(default_known_hosts_path)
    }
}

impl Default for HostKeyConfig {
    fn default() -> Self {
        Self::new(HostKeyPolicy::default())
    }
}

/// 기본 known_hosts 경로 (`~/.ssh/known_hosts`). HOME 이 없으면 `None`.
pub fn default_known_hosts_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".ssh").join("known_hosts"))
}

// ── 호스트 키 사전 수집 (keyscan) ────────────────────────────────────────
//
// 로드맵 #39 — 대규모 인프라를 `--host-key-policy strict`로 배포하려면 각
// 호스트의 known_hosts 항목이 미리 채워져 있어야 한다(그렇지 않으면 Strict가
// 모든 최초 연결을 거부한다). 이 모듈은 `ssh-keyscan`과 같은 목적을 수행한다
// — 실제 인증 없이 서버가 제시하는 호스트 공개키만 수집해, 운영자가
// 대역 밖(out-of-band) 채널(클라우드 콘솔 시리얼 로그, 프로비저닝 스크립트
// 출력 등)로 지문을 검증한 뒤 `known_hosts`에 반영할 수 있게 한다.
//
// **TOFU와는 목적이 다르다**: TOFU는 "첫 연결을 그냥 신뢰"하지만, 이 스캔은
// 키를 사람이 검증하기 좋은 형태(지문)로 노출하는 것이 목표다. 스캔 결과를
// 자동으로 `known_hosts`에 쓰는 것(`--write`)은 TOFU와 동일한 신뢰 모델이므로,
// 진짜 MITM 방어 효과를 얻으려면 반드시 지문을 out-of-band로 대조해야 한다.

/// 스캔된 단일 호스트 키. `ssh` 피처 여부와 무관하게 사용 가능한 순수 데이터 타입.
#[derive(Debug, Clone)]
pub struct ScannedHostKey {
    pub host: String,
    pub port: u16,
    /// 키 알고리즘 (예: `ssh-ed25519`, `rsa-sha2-512`).
    pub algorithm: String,
    /// `SHA256:<base64>` 형식 지문 — 운영자가 대역 밖 채널과 대조하는 값.
    pub fingerprint: String,
    /// `known_hosts` 파일에 그대로 append 가능한 한 줄
    /// (`host algo base64` 또는 포트가 22가 아니면 `[host]:port algo base64`).
    pub known_hosts_line: String,
}

/// 스캔된 known_hosts 한 줄을 파일에 append. `ssh` 피처와 무관하게 동작하는
/// 순수 파일 I/O — 다른 도구가 만든 known_hosts 줄을 붙일 때도 재사용 가능.
///
/// 중복 검사는 하지 않는다 — OpenSSH 클라이언트는 known_hosts에서 첫 매치를
/// 사용하므로 안전하지만, 파일을 깔끔하게 유지하고 싶다면 운영자가 직접
/// 정리해야 한다.
pub fn append_known_hosts_line(line: &str, path: &std::path::Path) -> Result<(), SshError> {
    use std::io::{Read, Seek, SeekFrom, Write};

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(path)?;

    // 기존 파일이 비어있지 않은데 개행으로 끝나지 않으면 먼저 개행을 넣어,
    // 새 줄이 기존 줄과 붙어버리는 것을 방지. 빈 파일(신규 생성 포함)에는
    // 선행 개행을 넣지 않는다 — `learn_known_hosts_path`의 동일 로직은
    // 빈 파일에서도 선행 개행을 넣는 사소한 흠이 있어 여기서는 피했다.
    let is_empty = file.metadata()?.len() == 0;
    let mut buf = [0u8; 1];
    let mut ends_in_newline = is_empty;
    if !is_empty && file.seek(SeekFrom::End(-1)).is_ok() {
        file.read_exact(&mut buf)?;
        ends_in_newline = buf[0] == b'\n';
    }
    file.seek(SeekFrom::End(0))?;

    let mut file = std::io::BufWriter::new(file);
    if !ends_in_newline {
        file.write_all(b"\n")?;
    }
    writeln!(file, "{line}")?;
    Ok(())
}

// ── russh 기반 실제 SSH 클라이언트 ─────────────────────────────────────

#[cfg(feature = "ssh")]
mod russh_impl {
    use super::*;
    use russh::{client, ChannelMsg};
    use russh_keys::key;
    use std::sync::Arc as StdArc;
    use tokio::sync::Mutex as TokioMutex;

    /// russh 클라이언트 인증 핸들러. `check_server_key`에서 호스트 키 검증 정책을 적용.
    ///
    /// 검증 정책(`HostKeyPolicy`)에 따라 `~/.ssh/known_hosts` 파일을 읽고
    /// 서버가 제시한 공개키를 비교한다. 검증에 실패하면 거부 사유를
    /// `reject_reason`에 기록하여 `SshClient::connect` 호출자에게 전달한다.
    pub struct SshClient {
        info: SshConnectInfo,
        session: TokioMutex<Option<client::Handle<SshHandler>>>,
    }

    /// russh 핸들러 상태. 호스트 키 검증은 `HostKeyPolicy` + known_hosts 경로로 결정.
    pub struct SshHandler {
        policy: HostKeyPolicy,
        host: String,
        port: u16,
        known_hosts_path: PathBuf,
        /// 검증 실패 사유를 connect 호출자에게 전달하기 위한 공유 슬롯.
        reject_reason: StdArc<std::sync::Mutex<Option<String>>>,
    }

    #[async_trait]
    impl client::Handler for SshHandler {
        type Error = russh::Error;

        async fn check_server_key(
            &mut self,
            server_public_key: &key::PublicKey,
        ) -> Result<bool, Self::Error> {
            match self.policy {
                HostKeyPolicy::AcceptAll => {
                    tracing::warn!(
                        host = %self.host,
                        "accepting server host key WITHOUT verification (accept-all policy; insecure)"
                    );
                    return Ok(true);
                }
                HostKeyPolicy::Strict | HostKeyPolicy::Tofu => {}
            }

            let path = &self.known_hosts_path;
            match russh_keys::check_known_hosts_path(&self.host, self.port, server_public_key, path)
            {
                Ok(true) => Ok(true), // 호스트가 known_hosts에 있고 키 일치
                Ok(false) => {
                    // 호스트가 known_hosts에 없음
                    if self.policy == HostKeyPolicy::Tofu {
                        tracing::info!(
                            host = %self.host,
                            known_hosts = %path.display(),
                            "first connection — learning host key (TOFU)"
                        );
                        match russh_keys::known_hosts::learn_known_hosts_path(
                            &self.host,
                            self.port,
                            server_public_key,
                            path,
                        ) {
                            Ok(()) => Ok(true),
                            Err(e) => {
                                self.set_reject(format!(
                                    "failed to write host key for '{host}' to {path}: {e}",
                                    host = self.host,
                                    path = path.display()
                                ));
                                Ok(false)
                            }
                        }
                    } else {
                        // Strict: 호스트가 없으면 거부
                        self.set_reject(format!(
                            "host '{host}' not found in known_hosts ({path}); \
                             use --host-key-policy tofu to auto-add on first connection",
                            host = self.host,
                            path = path.display()
                        ));
                        Ok(false)
                    }
                }
                Err(e) => {
                    // 키 불일치 (또는 파일 읽기 에러) — MITM 의심
                    self.set_reject(format!(
                        "host key mismatch for '{host}' (possible MITM): {e}",
                        host = self.host
                    ));
                    Ok(false)
                }
            }
        }
    }

    impl SshHandler {
        fn set_reject(&self, reason: String) {
            tracing::error!(%reason);
            *self.reject_reason.lock().unwrap() = Some(reason);
        }
    }

    impl SshClient {
        /// SSH 서버에 접속.
        ///
        /// `host_key` 구성에 따라 서버 호스트 키를 검증한다:
        /// - `AcceptAll`: 검증 생략 (위험, 자동화/테스트 전용)
        /// - `Tofu`: 첫 연결 시 known_hosts에 키 추가, 이후 일치 검사 (기본값)
        /// - `Strict`: known_hosts에 반드시 있어야 함
        pub async fn connect(
            info: SshConnectInfo,
            host_key: HostKeyConfig,
        ) -> Result<Self, SshError> {
            let config = StdArc::new(client::Config::default());
            let reject_reason = StdArc::new(std::sync::Mutex::new(None));

            let known_hosts_path =
                host_key
                    .effective_known_hosts()
                    .ok_or_else(|| SshError::HostKeyVerification {
                        host: info.host.clone(),
                        reason:
                            "HOME environment variable not set; cannot resolve known_hosts path. \
                             Pass --known-hosts explicitly or use --host-key-policy accept-all."
                                .into(),
                    })?;

            let handler = SshHandler {
                policy: host_key.policy,
                host: info.host.clone(),
                port: info.port,
                known_hosts_path,
                reject_reason: reject_reason.clone(),
            };

            let key_pair = russh_keys::load_secret_key(&info.key_path, None)
                .map_err(|e| SshError::KeyLoad(format!("{e}")))?;

            let addr = (info.host.as_str(), info.port);
            let mut session = match client::connect(config.clone(), addr, handler).await {
                Ok(s) => s,
                Err(e) => {
                    // 호스트 키 검증 거부인지 확인
                    if let Some(reason) = reject_reason.lock().unwrap().take() {
                        return Err(SshError::HostKeyVerification {
                            host: info.host.clone(),
                            reason,
                        });
                    }
                    return Err(SshError::Protocol(format!("connect: {e}")));
                }
            };

            let auth_ok = session
                .authenticate_publickey(&info.user, StdArc::new(key_pair))
                .await
                .map_err(|e| SshError::Protocol(format!("auth: {e}")))?;

            if !auth_ok {
                return Err(SshError::AuthFailed(info.user.clone()));
            }

            tracing::info!(
                host = %info.host,
                port = %info.port,
                user = %info.user,
                policy = %host_key.policy,
                "SSH connected"
            );

            Ok(Self {
                info,
                session: TokioMutex::new(Some(session)),
            })
        }

        pub fn connect_info(&self) -> &SshConnectInfo {
            &self.info
        }

        /// 활성 세션 핸들 레퍼런스 반환. 연결 끊김 시 에러.
        /// Mutex 가드를 반환하므로 호출자는 가드를 든 채로 메서드 호출.
        async fn session_ref<'a>(
            &'a self,
        ) -> Result<tokio::sync::MutexGuard<'a, Option<client::Handle<SshHandler>>>, SshError>
        {
            let guard = self.session.lock().await;
            if guard.is_none() {
                return Err(SshError::NotConnected);
            }
            Ok(guard)
        }
    }

    /// 서버가 제시하는 SSH 호스트 공개키만 수집. 인증은 하지 않는다
    /// (`ssh-keyscan`과 동일한 목적 — `ScannedHostKey`/`append_known_hosts_line`
    /// 문서 참고).
    ///
    /// 구현: `check_server_key` 콜백에서 키를 캡처한 뒤 `Ok(false)`를 반환해
    /// handshake를 즉시 종료시킨다 — 개인키·사용자 계정 없이도 키를 얻을 수
    /// 있다(실제 인증 단계까지 갈 필요가 없으므로).
    pub async fn scan_host_key(host: &str, port: u16) -> Result<ScannedHostKey, SshError> {
        struct CaptureHandler {
            captured: StdArc<std::sync::Mutex<Option<key::PublicKey>>>,
        }

        #[async_trait]
        impl client::Handler for CaptureHandler {
            type Error = russh::Error;

            async fn check_server_key(
                &mut self,
                server_public_key: &key::PublicKey,
            ) -> Result<bool, Self::Error> {
                *self.captured.lock().unwrap() = Some(server_public_key.clone());
                // 스캔이 목적이므로 실제 연결/인증까지 진행하지 않고 즉시 거부.
                Ok(false)
            }
        }

        let captured = StdArc::new(std::sync::Mutex::new(None));
        let handler = CaptureHandler {
            captured: captured.clone(),
        };
        let config = StdArc::new(client::Config::default());

        // 위 핸들러가 항상 `Ok(false)`를 반환하므로 이 connect는 (키를 이미
        // 캡처했더라도) 대개 host-key-rejected 에러로 끝난다 — 의도된 동작.
        let connect_result = client::connect(config, (host, port), handler).await;

        let key = captured.lock().unwrap().take().ok_or_else(|| {
            let detail = connect_result
                .as_ref()
                .err()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "connection unexpectedly succeeded".to_string());
            SshError::Protocol(format!(
                "no host key observed while scanning {host}:{port}: {detail}"
            ))
        })?;

        let mut buf = Vec::new();
        russh_keys::known_hosts::write_public_key_base64(&mut buf, &key)
            .map_err(SshError::RusshKeys)?;
        let algo_and_b64 = String::from_utf8_lossy(&buf).trim_end().to_string();

        let known_hosts_line = if port == 22 {
            format!("{host} {algo_and_b64}")
        } else {
            format!("[{host}]:{port} {algo_and_b64}")
        };

        Ok(ScannedHostKey {
            host: host.to_string(),
            port,
            algorithm: key.name().to_string(),
            fingerprint: format!("SHA256:{}", key.fingerprint()),
            known_hosts_line,
        })
    }

    #[async_trait]
    impl RemoteExecutor for SshClient {
        async fn exec(&self, command: &str) -> Result<String, SshError> {
            let guard = self.session_ref().await?;
            let session = guard.as_ref().unwrap();
            let mut channel = session
                .channel_open_session()
                .await
                .map_err(|e| SshError::Protocol(format!("open channel: {e}")))?;

            channel
                .exec(true, command)
                .await
                .map_err(|e| SshError::Protocol(format!("exec: {e}")))?;

            let mut output = Vec::new();
            let mut exit_code: i32 = 0;
            while let Some(msg) = channel.wait().await {
                match msg {
                    ChannelMsg::Data { ref data } => output.extend_from_slice(data),
                    ChannelMsg::ExtendedData { ref data, .. } => output.extend_from_slice(data),
                    ChannelMsg::ExitStatus { exit_status } => {
                        exit_code = exit_status as i32;
                    }
                    _ => {}
                }
            }

            if exit_code != 0 {
                tracing::debug!(exit_code, %command, "remote command non-zero exit");
            }
            Ok(String::from_utf8_lossy(&output).into_owned())
        }

        async fn exec_streaming(
            &self,
            command: &str,
            mut on_line: Box<dyn for<'a> FnMut(&'a str) + Send>,
        ) -> Result<i32, SshError> {
            let guard = self.session_ref().await?;
            let session = guard.as_ref().unwrap();
            let mut channel = session
                .channel_open_session()
                .await
                .map_err(|e| SshError::Protocol(format!("open channel: {e}")))?;

            channel
                .exec(true, command)
                .await
                .map_err(|e| SshError::Protocol(format!("exec: {e}")))?;

            let mut buf: Vec<u8> = Vec::new();
            let mut exit_code: i32 = 0;
            while let Some(msg) = channel.wait().await {
                match msg {
                    ChannelMsg::Data { ref data } => {
                        buf.extend_from_slice(data);
                        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                            let line: Vec<u8> = buf.drain(..=pos).collect();
                            let trimmed = String::from_utf8_lossy(&line);
                            let trimmed = trimmed.trim_end();
                            on_line(trimmed);
                        }
                    }
                    ChannelMsg::ExtendedData { ref data, .. } => {
                        buf.extend_from_slice(data);
                        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                            let line: Vec<u8> = buf.drain(..=pos).collect();
                            let trimmed = String::from_utf8_lossy(&line);
                            let trimmed = trimmed.trim_end();
                            on_line(trimmed);
                        }
                    }
                    ChannelMsg::ExitStatus { exit_status } => {
                        exit_code = exit_status as i32;
                    }
                    _ => {}
                }
            }
            if !buf.is_empty() {
                let trimmed = String::from_utf8_lossy(&buf);
                on_line(trimmed.trim_end());
            }
            Ok(exit_code)
        }

        async fn upload_file(
            &self,
            local_path: &str,
            remote_path: &str,
            mode: u32,
        ) -> Result<(), SshError> {
            let data = tokio::fs::read(local_path).await?;
            use base64::{engine::general_purpose::STANDARD, Engine as _};
            let b64 = STANDARD.encode(&data);
            self.exec(&format!(
                "echo '{b64}' | base64 -d > {remote_path} && chmod {mode:o} {remote_path}",
            ))
            .await?;
            Ok(())
        }

        async fn write_file(&self, path: &str, content: &str) -> Result<(), SshError> {
            use base64::{engine::general_purpose::STANDARD, Engine as _};
            let b64 = STANDARD.encode(content.as_bytes());
            self.exec(&format!("echo '{b64}' | base64 -d > {path}"))
                .await?;
            Ok(())
        }
    }
}

#[cfg(feature = "ssh")]
pub use russh_impl::{scan_host_key, SshClient, SshHandler};

#[cfg(not(feature = "ssh"))]
mod stub {
    //! `ssh` feature가 비활성화된 경우 `SshClient` 타입을 노출하지 않음.
    //! 라이브러리 사용자는 `MockExecutor` 또는 직접 `RemoteExecutor` 구현체 사용.
    use super::*;
    use crate::error::SshError;

    /// SSH feature가 비활성화된 경우의 자리표시자 타입.
    /// `connect()` 호출 시 항상 에러 반환.
    pub struct SshClient;

    impl SshClient {
        pub async fn connect(
            _info: SshConnectInfo,
            _host_key: HostKeyConfig,
        ) -> Result<Self, SshError> {
            Err(SshError::Protocol(
                "SSH support is disabled. Rebuild with `--features ssh`.".into(),
            ))
        }
    }

    /// `ssh` feature가 비활성화된 경우 항상 에러.
    pub async fn scan_host_key(_host: &str, _port: u16) -> Result<ScannedHostKey, SshError> {
        Err(SshError::Protocol(
            "SSH support is disabled. Rebuild with `--features ssh`.".into(),
        ))
    }
}

#[cfg(not(feature = "ssh"))]
pub use stub::{scan_host_key, SshClient};

// ── MockExecutor (테스트용) ─────────────────────────────────────────────

/// 사전 프로그래밍된 응답을 반환하는 인메모리 `RemoteExecutor`.
///
/// `expect_exec(command, response)`로 명령별 응답을 등록. 매칭되지 않은
/// 명령은 빈 문자열 반환. `recorded_calls()`로 실행된 명령 기록 조회.
pub struct MockExecutor {
    responses: Mutex<HashMap<String, String>>,
    exit_codes: Mutex<HashMap<String, i32>>,
    calls: Mutex<Vec<String>>,
}

impl MockExecutor {
    pub fn new() -> Self {
        Self {
            responses: Mutex::new(HashMap::new()),
            exit_codes: Mutex::new(HashMap::new()),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// `command` 실행 시 `response` 반환하도록 프로그래밍.
    /// 정확한 문자열 매칭 (substring 아님).
    pub fn expect_exec(&self, command: impl Into<String>, response: impl Into<String>) {
        self.responses
            .lock()
            .unwrap()
            .insert(command.into(), response.into());
    }

    /// 특정 명령의 exit 코드 지정 (기본 0).
    pub fn expect_exit(&self, command: impl Into<String>, code: i32) {
        self.exit_codes.lock().unwrap().insert(command.into(), code);
    }

    /// 실행된 모든 명령 기록 조회 (호출 순서대로).
    pub fn recorded_calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    /// 응답이 등록되었는지 접두사 매칭으로 조회 (substring).
    fn lookup_response(&self, command: &str) -> String {
        let responses = self.responses.lock().unwrap();
        // 정확 매칭 우선
        if let Some(r) = responses.get(command) {
            return r.clone();
        }
        // 접두사 매칭 (플랜의 동적 명령 지원)
        for (key, val) in responses.iter() {
            if command.starts_with(key) {
                return val.clone();
            }
        }
        String::new()
    }

    fn lookup_exit(&self, command: &str) -> i32 {
        let exits = self.exit_codes.lock().unwrap();
        if let Some(c) = exits.get(command) {
            return *c;
        }
        for (key, val) in exits.iter() {
            if command.starts_with(key) {
                return *val;
            }
        }
        0
    }
}

impl Default for MockExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RemoteExecutor for MockExecutor {
    async fn exec(&self, command: &str) -> Result<String, SshError> {
        self.calls.lock().unwrap().push(command.to_string());
        Ok(self.lookup_response(command))
    }

    async fn exec_streaming(
        &self,
        command: &str,
        mut on_line: Box<dyn for<'a> FnMut(&'a str) + Send>,
    ) -> Result<i32, SshError> {
        self.calls.lock().unwrap().push(command.to_string());
        let response = self.lookup_response(command);
        for line in response.lines() {
            on_line(line);
        }
        Ok(self.lookup_exit(command))
    }

    async fn upload_file(
        &self,
        local_path: &str,
        remote_path: &str,
        _mode: u32,
    ) -> Result<(), SshError> {
        let cmd = format!("upload {local_path} → {remote_path}");
        self.calls.lock().unwrap().push(cmd);
        Ok(())
    }

    async fn write_file(&self, path: &str, content: &str) -> Result<(), SshError> {
        let cmd = format!("write {path} ({} bytes)", content.len());
        self.calls.lock().unwrap().push(cmd);
        Ok(())
    }
}

/// `Arc<dyn RemoteExecutor>` 생성 헬퍼.
pub fn arc_executor<E: RemoteExecutor + 'static>(executor: E) -> Arc<dyn RemoteExecutor> {
    Arc::new(executor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_executor_returns_programmed_response() {
        let exec = MockExecutor::new();
        exec.expect_exec("uname -m", "x86_64\n");
        let out = exec.exec("uname -m").await.unwrap();
        assert_eq!(out, "x86_64\n");
    }

    #[tokio::test]
    async fn mock_executor_records_calls_in_order() {
        let exec = MockExecutor::new();
        exec.exec("cmd1").await.unwrap();
        exec.exec("cmd2").await.unwrap();
        exec.exec("cmd3").await.unwrap();
        assert_eq!(exec.recorded_calls(), vec!["cmd1", "cmd2", "cmd3"]);
    }

    #[tokio::test]
    async fn mock_executor_prefix_matching() {
        let exec = MockExecutor::new();
        exec.expect_exec("cloudflared tunnel create", "ok\n");
        let out = exec
            .exec("cloudflared tunnel create fleet-build-1")
            .await
            .unwrap();
        assert_eq!(out, "ok\n");
    }

    #[tokio::test]
    async fn mock_executor_streaming_emits_lines() {
        let exec = MockExecutor::new();
        exec.expect_exec("build", "line1\nline2\nline3");
        let collected = Arc::new(Mutex::new(Vec::<String>::new()));
        let cloned = collected.clone();
        let code = exec
            .exec_streaming(
                "build",
                Box::new(move |line| {
                    cloned.lock().unwrap().push(line.to_string());
                }),
            )
            .await
            .unwrap();
        assert_eq!(code, 0);
        assert_eq!(*collected.lock().unwrap(), vec!["line1", "line2", "line3"]);
    }

    #[tokio::test]
    async fn mock_executor_write_file_records() {
        let exec = MockExecutor::new();
        exec.write_file("/tmp/x", "hello").await.unwrap();
        let calls = exec.recorded_calls();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].contains("/tmp/x"));
        assert!(calls[0].contains("5 bytes"));
    }

    #[tokio::test]
    async fn mock_executor_exit_code_lookup() {
        let exec = MockExecutor::new();
        exec.expect_exit("failing-cmd", 42);
        let code = exec
            .exec_streaming("failing-cmd", Box::new(|_| {}))
            .await
            .unwrap();
        assert_eq!(code, 42);
    }

    // ── HostKeyPolicy / HostKeyConfig 단위 테스트 ──────────────────────

    #[test]
    fn host_key_policy_default_is_tofu() {
        assert_eq!(HostKeyPolicy::default(), HostKeyPolicy::Tofu);
    }

    #[test]
    fn host_key_policy_parses_aliases() {
        // accept-all 별칭
        assert_eq!(
            HostKeyPolicy::parse("accept-all").unwrap(),
            HostKeyPolicy::AcceptAll
        );
        assert_eq!(
            HostKeyPolicy::parse("ACCEPT_ALL").unwrap(),
            HostKeyPolicy::AcceptAll
        );
        assert_eq!(
            HostKeyPolicy::parse("no").unwrap(),
            HostKeyPolicy::AcceptAll
        );
        assert_eq!(
            HostKeyPolicy::parse("insecure").unwrap(),
            HostKeyPolicy::AcceptAll
        );

        // tofu 별칭
        assert_eq!(HostKeyPolicy::parse("tofu").unwrap(), HostKeyPolicy::Tofu);
        assert_eq!(
            HostKeyPolicy::parse("accept-new").unwrap(),
            HostKeyPolicy::Tofu
        );
        assert_eq!(HostKeyPolicy::parse("auto").unwrap(), HostKeyPolicy::Tofu);

        // strict 별칭
        assert_eq!(
            HostKeyPolicy::parse("strict").unwrap(),
            HostKeyPolicy::Strict
        );
        assert_eq!(HostKeyPolicy::parse("yes").unwrap(), HostKeyPolicy::Strict);
    }

    #[test]
    fn host_key_policy_rejects_unknown() {
        assert!(HostKeyPolicy::parse("bogus").is_err());
        let err = HostKeyPolicy::parse("lax").unwrap_err();
        assert!(err.contains("unknown host key policy"));
        assert!(err.contains("accept-all"));
        assert!(err.contains("tofu"));
        assert!(err.contains("strict"));
    }

    #[test]
    fn host_key_policy_display_roundtrips_as_str() {
        for p in [
            HostKeyPolicy::AcceptAll,
            HostKeyPolicy::Tofu,
            HostKeyPolicy::Strict,
        ] {
            let s = p.to_string();
            assert_eq!(s, p.as_str());
            // as_str 값은 parse 로 왕복 가능해야 함
            assert_eq!(HostKeyPolicy::parse(&s).unwrap(), p);
        }
    }

    #[test]
    fn host_key_config_default_is_tofu_with_no_explicit_path() {
        let cfg = HostKeyConfig::default();
        assert_eq!(cfg.policy, HostKeyPolicy::Tofu);
        assert!(cfg.known_hosts_path.is_none());
    }

    #[test]
    fn host_key_config_builder_sets_known_hosts() {
        let cfg = HostKeyConfig::new(HostKeyPolicy::Strict).with_known_hosts("/tmp/kh");
        assert_eq!(cfg.policy, HostKeyPolicy::Strict);
        assert_eq!(
            cfg.known_hosts_path.as_deref().unwrap().to_string_lossy(),
            "/tmp/kh"
        );
    }

    #[test]
    fn effective_known_hosts_prefers_explicit_path() {
        // 명시 경로가 있으면 HOME 기반 기본 경로보다 우선.
        let cfg = HostKeyConfig::new(HostKeyPolicy::Strict).with_known_hosts("/explicit/kh");
        let eff = cfg.effective_known_hosts().unwrap();
        assert_eq!(eff, PathBuf::from("/explicit/kh"));
    }

    #[test]
    fn effective_known_hosts_falls_back_to_home_default() {
        // HOME 이 설정된 경우 ~/.ssh/known_hosts 로 폴백.
        // (CI 등 HOME 미설정 환경에서는 이 테스트를 건너뜀.)
        if std::env::var_os("HOME").is_none() {
            eprintln!("skipping: HOME not set");
            return;
        }
        let cfg = HostKeyConfig::new(HostKeyPolicy::Tofu);
        let eff = cfg.effective_known_hosts().expect("HOME set");
        let s = eff.to_string_lossy();
        assert!(
            s.ends_with("/.ssh/known_hosts"),
            "expected ~/.ssh/known_hosts, got {s}"
        );
    }

    // ── append_known_hosts_line (로드맵 #39) ────────────────────────────

    #[test]
    fn append_known_hosts_line_creates_file_and_parent_dirs() {
        let dir = std::env::temp_dir().join(format!("fleet-test-kh-{}", uuid_like()));
        let path = dir.join("known_hosts");
        assert!(!dir.exists());

        append_known_hosts_line("10.0.0.5 ssh-ed25519 AAAA...", &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "10.0.0.5 ssh-ed25519 AAAA...\n");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn append_known_hosts_line_appends_without_clobbering() {
        let dir = std::env::temp_dir().join(format!("fleet-test-kh-{}", uuid_like()));
        let path = dir.join("known_hosts");

        append_known_hosts_line("host-a ssh-ed25519 AAAA", &path).unwrap();
        append_known_hosts_line("host-b ssh-ed25519 BBBB", &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            content,
            "host-a ssh-ed25519 AAAA\nhost-b ssh-ed25519 BBBB\n"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn append_known_hosts_line_inserts_newline_if_file_lacked_trailing_one() {
        let dir = std::env::temp_dir().join(format!("fleet-test-kh-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("known_hosts");
        // 개행 없이 끝나는 기존 파일을 시뮬레이션.
        std::fs::write(&path, "host-a ssh-ed25519 AAAA").unwrap();

        append_known_hosts_line("host-b ssh-ed25519 BBBB", &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            content,
            "host-a ssh-ed25519 AAAA\nhost-b ssh-ed25519 BBBB\n"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 병렬 테스트 실행 시 임시 파일 경로 충돌을 피하기 위한 저비용 유사-UUID.
    /// 실제 UUID 크레이트를 새로 끌어오지 않고 시간+스레드 ID로 충분히 유일하게 만든다.
    fn uuid_like() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{nanos:x}-{:?}", std::thread::current().id())
    }
}
