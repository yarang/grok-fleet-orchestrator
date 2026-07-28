//! 호스트 인벤토리 도메인 모델.
//!
//! `workers` 테이블이 "현재 등록된 워커"만 추적하는 반면,
//! `hosts`는 인프라에 존재하는 모든 호스트를 추적한다.
//! 프로비저닝 대기, 미등록, 장애 호스트 등을 포함한다.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::ids::WorkerId;
use crate::worker::OsInfo;

/// 호스트 엔티티.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Host {
    pub id: Uuid,
    pub hostname: String,
    /// 연결된 워커 ID (등록된 경우).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<WorkerId>,
    pub status: HostStatus,
    /// SSH 접속 정보.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_host: Option<String>,
    #[serde(default = "default_ssh_port")]
    pub ssh_port: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_user: Option<String>,
    /// 런타임 정보 (heartbeat로 갱신).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grok_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fleet_worker_version: Option<String>,
    #[serde(default)]
    pub os_info: Option<OsInfo>,
    /// 시스템 메트릭 스냅샷.
    #[serde(default)]
    pub metrics: HostMetrics,
    /// 타임스탬프.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provisioned_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn default_ssh_port() -> i32 {
    22
}

/// 호스트 메트릭 스냅샷 (heartbeat로 갱신).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostMetrics {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub load_avg: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mem_available_mb: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_free_mb: Option<u64>,
}

/// 호스트 가용성 상태.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostStatus {
    /// 프로비저닝 완료, 워커 등록 대기 중.
    #[default]
    Provisioned,
    /// 온라인 — heartbeat 수신 중.
    Online,
    /// 오프라인 — heartbeat 누락.
    Offline,
    /// 프로비저닝 실패 또는 치명적 장애.
    Failed,
}

/// 호스트 이벤트 (타임라인).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostEvent {
    pub id: Uuid,
    pub host_id: Uuid,
    pub event_type: String,
    pub severity: EventSeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub payload: HashMap<String, serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

/// 이벤트 심각도.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSeverity {
    #[default]
    Info,
    Warn,
    Error,
}

impl EventSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

impl HostStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Provisioned => "provisioned",
            Self::Online => "online",
            Self::Offline => "offline",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "provisioned" => Some(Self::Provisioned),
            "online" => Some(Self::Online),
            "offline" => Some(Self::Offline),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

// ── SSH 키 금고 ─────────────────────────────────────────────────────────

/// 암호화 저장된 SSH 비밀키 레코드.
///
/// `encrypted_blob`은 `MasterKey` (AES-256-GCM)로 암호화된 OpenSSH 비밀키.
/// 복호화는 프로비저닝 실행 시에만 수행.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshKey {
    pub id: Uuid,
    pub name: String,
    /// AES-256-GCM 암호화된 비밀키 (base64url).
    #[serde(skip_serializing)]
    pub encrypted_blob: String,
    /// 공개키 fingerprint (SHA-256 base64).
    pub fingerprint: String,
    /// 키 타입: "ed25519", "rsa", "ecdsa".
    pub key_type: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// SSH 키 목록 API용 요약 (비밀키 제외).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshKeySummary {
    pub name: String,
    pub fingerprint: String,
    pub key_type: String,
    pub created_at: DateTime<Utc>,
}
