//! 타입 안전 식별자 (newtype 패턴).
//!
//! `TaskId`와 `WorkerId`는 모두 `Uuid` 기반이지만, 서로 다른 타입으로
//! 선언하여 컴파일 타임에 혼용을 방지합니다. 예를 들어 `get_task(worker_id)`
//! 같은 실수를 컴파일러가 잡아냅니다.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 작업 식별자. `TaskId::new()`로 새로 발급.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct TaskId(pub Uuid);

/// 워커 식별자. `WorkerId::new()`로 새로 발급.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct WorkerId(pub Uuid);

/// 프로젝트 식별자. 아직 project 기능은 없지만, `Task.project_id`가 나중에
/// project 도입 시 재귀 backfill 없이 바로 채워질 수 있도록 타입만 미리
/// 예약해둔다 — [`TaskId`]/[`WorkerId`]와 동일한 newtype 패턴.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct ProjectId(pub Uuid);

// ── TaskId ───────────────────────────────────────────────────────────────

impl TaskId {
    /// 새 무작위 `TaskId`를 발급합니다 (UUIDv4).
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// nil UUID (모두 0). 테스트나 플레이스홀더용.
    pub fn nil() -> Self {
        Self(Uuid::nil())
    }

    /// 내부 `Uuid` 반환.
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for TaskId {
    fn from(u: Uuid) -> Self {
        Self(u)
    }
}

impl FromStr for TaskId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl Serialize for TaskId {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(ser)
    }
}

impl<'de> Deserialize<'de> for TaskId {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        Uuid::deserialize(de).map(Self)
    }
}

// ── WorkerId ─────────────────────────────────────────────────────────────

impl WorkerId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn nil() -> Self {
        Self(Uuid::nil())
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for WorkerId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for WorkerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for WorkerId {
    fn from(u: Uuid) -> Self {
        Self(u)
    }
}

impl FromStr for WorkerId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl Serialize for WorkerId {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(ser)
    }
}

impl<'de> Deserialize<'de> for WorkerId {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        Uuid::deserialize(de).map(Self)
    }
}

// ── ProjectId ────────────────────────────────────────────────────────────

impl ProjectId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for ProjectId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for ProjectId {
    fn from(u: Uuid) -> Self {
        Self(u)
    }
}

impl FromStr for ProjectId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl Serialize for ProjectId {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(ser)
    }
}

impl<'de> Deserialize<'de> for ProjectId {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        Uuid::deserialize(de).map(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_and_worker_ids_are_distinct_types() {
        // 다른 타입이므로 직접 비교 불가 — 컴파일 타임 보장.
        let t = TaskId::new();
        let w = WorkerId::new();
        assert_eq!(format!("{t}").len(), 36);
        assert_eq!(format!("{w}").len(), 36);
    }

    #[test]
    fn roundtrip_string_parse() {
        let id = TaskId::new();
        let s = id.to_string();
        let parsed: TaskId = s.parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn serialize_as_plain_uuid_string() {
        let id = WorkerId::new();
        let json = serde_json::to_string(&id).unwrap();
        // JSON 문자열 형태 ("...")로 직렬화
        assert!(json.starts_with('"') && json.ends_with('"'));
        let back: WorkerId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }
}
