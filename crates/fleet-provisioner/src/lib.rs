//! # fleet-provisioner
//!
//! SSH 기반 원격 프로비저닝 자동화. `russh`(순수 러스트 SSH2 구현)로 워커 머신에
//! 접속하여 일련의 멱등 스텝을 순차 실행합니다.
//!
//! ## 설계 원칙
//!
//! 1. **단일 바이너리 배포**: Python/Ansible 불필요. `fleet provision` 한 명령.
//! 2. **스트리밍 로그**: 원격 stdout/stderr를 라인 단위로 콜백 전달.
//! 3. **멱등성**: 각 스텝은 `is_applied()`로 이미 적용되었는지 검사.
//! 4. **테스트 용이성**: `RemoteExecutor` 트레이트로 SSH를 추상화.
//!    러스트 테스트는 `MockExecutor`로 실제 SSH 없이 스텝 로직 검증.
//!
//! ## 모듈 구성
//!
//! - `ssh` — `SshClient`(russh)와 `RemoteExecutor` 트레이트, `MockExecutor`.
//! - `steps` — `Step` 트레이트와 개별 스텝(check_prereqs, install_deps, …).
//! - `playbook` — `Playbook` 시퀀스 실행기.
//! - `inventory` — `workers.yaml` 파싱 (단일/일괄 프로비저닝).

#![forbid(unsafe_code)]
#![allow(missing_docs)]

pub mod error;
pub mod inventory;
pub mod playbook;
pub mod ssh;
pub mod steps;
pub mod templates;

pub use error::{InventoryError, PlaybookError, ProvisionError, SshError, StepError};
pub use inventory::{Inventory, InventoryDefaults, InventoryWorker, ProvisionOptions};
pub use playbook::{Playbook, PlaybookContext, PlaybookReport, StepStatus};
pub use ssh::{MockExecutor, RemoteExecutor, SshClient, SshConnectInfo};
pub use steps::{
    check_prereqs::CheckPrereqs, install_cloudflared::InstallCloudflared,
    install_deps::InstallDeps, install_fleet_worker::InstallFleetWorker, install_grok::InstallGrok,
    start_services::StartServices, PrereqReport, Step, StepContext, StepOutput, TunnelInfo,
};

pub use templates::TemplateContext;
