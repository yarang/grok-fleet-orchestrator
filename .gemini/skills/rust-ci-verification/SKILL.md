---
name: rust-ci-verification
description: Guidelines for resolving Rust conditional compilation (no-default-features) name resolution issues and clippy items_after_test_module warnings under GitHub Actions CI.
---

# Rust CI 검증 및 피처 게이트 해결 지침 (Rust CI Verification & Feature Gate Skill)

이 스킬은 Rust 프로젝트에서 조건부 컴파일(Feature Gates) 및 Clippy 정적 검사 연동 중 발생하는 일반적인 빌드 실패 양상을 빠르게 진단하고 해결하는 자동화 지침서입니다.

---

## 1. 조건부 컴파일 네임 리졸브 버그 해결 (Conditional Compilation Name Resolution)

### 🚨 문제 상황 (Problem)
의존성 크레이트는 Cargo.toml에 존재하지만 특정 피처 게이트(`#[cfg(feature = "...")]`)에 의해 활성화/비활성화되는 경우, 비활성화 빌드 시점에 네임스페이스 해석기(Name Resolver)의 버그 또는 충돌로 인해 다음과 같은 컴파일 오류가 발생할 수 있습니다.
```text
error[E0433]: failed to resolve: use of undeclared crate or module `fleet_transport`
```

### 💡 해결 가이드라인 (Solution)
1.  **크레이트 접두사(Prefix) 제거**: 
    코드 구현부에서 크레이트 명을 네임스페이스 접두사로 직접 쓰지 마십시오 (`fleet_transport::AcpTransport::new()` X).
2.  **조건부 상단 임포트**: 
    크레이트 및 해당 타입만 파일 최상단에서 조건부 컴파일 가드로 use 바인딩을 수행한 후, 코드부에서는 직접 타입명만 사용하도록 리팩토링하십시오.
    
```rust
// [Good Practice]
// 1. 상단 임포트 시 조건부 바인딩 수행
#[cfg(feature = "acp")]
use fleet_transport::AcpTransport;

// 2. 구현 코드 내에서는 접두사 없이 사용
#[cfg(feature = "acp")]
fn build_transport() -> Result<AcpTransport, Error> {
    let transport = AcpTransport::new();
    Ok(transport)
}
```

---

## 2. Clippy 테스트 모듈 순서 경고 해결 (Clippy items_after_test_module)

### 🚨 문제 상황 (Problem)
`#[cfg(test)] mod tests` 블록 하단이나 뒤에 일반 비즈니스 로직 함수 또는 공개 헬퍼 함수가 배치되면 Clippy는 정적 코드 가독성 저해로 판단해 빌드 경고를 내뿜고 CI가 실패합니다.
```text
warning: items after a test module
```

### 💡 해결 가이드라인 (Solution)
*   **테스트 모듈 최하단 배치**:
    `mod tests` 블록은 파일 내 다른 모든 구현체(함수, 구조체, 미들웨어 등)가 완전히 끝난 뒤 **항상 소스 코드 파일의 가장 마지막(최하단)**에 위치해야 합니다. 테스트 뒤에 일반 코드가 놓이지 않도록 소스 위치 구조를 정렬하십시오.

---

## 3. 원격 Push 전 로컬 자가 진단 프로토콜 (Pre-push Check)

코드를 푸시하기 전에 로컬 개발 환경에서 다음 2가지 검증 과정을 필수적으로 이행하여 CI 파이프라인의 회복 탄력성을 유지합니다.

```bash
# Step 1: 최소 기능(Feature-off) 환경 컴파일 검증
cargo check --no-default-features

# Step 2: 전체 피처 하의 Clippy 정적 에러/경고 전수 검증
cargo clippy --all-targets --all-features
```
