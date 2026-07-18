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

// ── russh 기반 실제 SSH 클라이언트 ─────────────────────────────────────

#[cfg(feature = "ssh")]
mod russh_impl {
    use super::*;
    use russh::{client, ChannelMsg};
    use russh_keys::key;
    use std::sync::Arc as StdArc;
    use tokio::sync::Mutex as TokioMutex;

    /// russh 인증 핸들러. 서버 공개키 검증 정책은 여기서 결정.
    ///
    /// 기본 정책은 `accept_all`(known_hosts 미검증)이지만 프로덕션에서는
    /// `Strict` 모드를 활성화해야 합니다. TODO: `~/.ssh/known_hosts` 연동.
    pub struct SshClient {
        info: SshConnectInfo,
        session: TokioMutex<Option<client::Handle<SshHandler>>>,
    }

    /// russh 핸들러 상태. 현재는 서버키 무조건 수용 (TODO: known_hosts).
    pub struct SshHandler {
        pub strict_host_key: bool,
    }

    #[async_trait]
    impl client::Handler for SshHandler {
        type Error = russh::Error;

        async fn check_server_key(
            &mut self,
            _server_public_key: &key::PublicKey,
        ) -> Result<bool, Self::Error> {
            if self.strict_host_key {
                // TODO: known_hosts와 비교
                tracing::warn!(
                    "strict_host_key=true but known_hosts check not yet implemented; accepting key"
                );
            }
            Ok(true)
        }
    }

    impl SshClient {
        /// SSH 서버에 접속. `key_path`는 개인키 파일 경로.
        pub async fn connect(info: SshConnectInfo) -> Result<Self, SshError> {
            let config = StdArc::new(client::Config::default());
            let handler = SshHandler {
                strict_host_key: false,
            };

            let key_pair = russh_keys::load_secret_key(&info.key_path, None)
                .map_err(|e| SshError::KeyLoad(format!("{e}")))?;

            let addr = (info.host.as_str(), info.port);
            let mut session = client::connect(config.clone(), addr, handler)
                .await
                .map_err(|e| SshError::Protocol(format!("connect: {e}")))?;

            let auth_ok = session
                .authenticate_publickey(&info.user, StdArc::new(key_pair))
                .await
                .map_err(|e| SshError::Protocol(format!("auth: {e}")))?;

            if !auth_ok {
                return Err(SshError::AuthFailed(info.user.clone()));
            }

            tracing::info!(
                host = %info.host,
                port = info.port,
                user = %info.user,
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
            self.exec(&format!("echo '{b64}' | base64 -d > {path}")).await?;
            Ok(())
        }
    }
}

#[cfg(feature = "ssh")]
pub use russh_impl::{SshClient, SshHandler};

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
        pub async fn connect(_info: SshConnectInfo) -> Result<Self, SshError> {
            Err(SshError::Protocol(
                "SSH support is disabled. Rebuild with `--features ssh`.".into(),
            ))
        }
    }
}

#[cfg(not(feature = "ssh"))]
pub use stub::SshClient;

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
        self.exit_codes
            .lock()
            .unwrap()
            .insert(command.into(), code);
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
        assert_eq!(
            *collected.lock().unwrap(),
            vec!["line1", "line2", "line3"]
        );
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
}
