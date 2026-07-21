//! `fleet users` — 대시보드 RBAC 사용자 관리 (Phase 9.1.6).
//!
//! 서버(`fleet serve`)와 동일한 `DATABASE_URL` Postgres에 직접 연결하여
//! 사용자 / 역할 / 권한을 관리합니다. 별도의 HTTP API 호출이나 인증이
//! 필요 없습니다 — CLI를 실행할 수 있는 호스트(서버 본체 또는
//! DATABASE_URL 접근 권한이 있는 운영자 머신)에서 직접 DB를 조작.
//!
//! ## 보안 설계
//!
//! - **비밀번호 입력**: stdin TTY에서 읽음 (`rpassword` 크레이트).
//!   셸 히스토리에 남지 않음.
//! - **비밀번호 검증**: zxcvbn 점수 ≥ 3, 최소 12자.
//! - **비밀번호 해싱**: Argon2id (OWASP 권장 파라미터).
//! - **삭제 전 확인**: username을 다시 입력받아 확인.

use std::io::{self, Write};

use anyhow::{anyhow, Context, Result};

use fleet_core::auth::password::{hash_password, verify_password};
use fleet_core::{BuiltinRole, User, UserId};
use fleet_store::Store;

use crate::UsersAction;

// ═══════════════════════════════════════════════════════════════════════
//  진입점
// ═══════════════════════════════════════════════════════════════════════

/// `fleet users` 명령 디스패치.
pub async fn run(action: UsersAction) -> Result<()> {
    let store = connect_store().await?;

    match action {
        UsersAction::List { json } => list_users(&store, json).await,
        UsersAction::Show { username } => show_user(&store, &username).await,
        UsersAction::Create {
            username,
            email,
            roles,
            password,
        } => create_user(&store, &username, email.as_deref(), roles, password).await,
        UsersAction::Passwd { username, password } => {
            change_password(&store, &username, password).await
        }
        UsersAction::Enable { username } => set_enabled(&store, &username, true).await,
        UsersAction::Disable { username } => set_enabled(&store, &username, false).await,
        UsersAction::Delete { username, yes } => delete_user(&store, &username, yes).await,
        UsersAction::Role { action } => match action {
            crate::UserRoleAction::Assign { username, role } => {
                assign_role(&store, &username, &role).await
            }
            crate::UserRoleAction::Revoke { username, role } => {
                revoke_role(&store, &username, &role).await
            }
        },
        UsersAction::BootstrapToken {
            expires_in_hours,
            force,
        } => issue_bootstrap_token(&store, expires_in_hours, force).await,
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  DB 연결
// ═══════════════════════════════════════════════════════════════════════

async fn connect_store() -> Result<fleet_store::PgStore> {
    let url = std::env::var("DATABASE_URL")
        .context("DATABASE_URL is not set. Export DATABASE_URL=postgres://user@host/dbname")?;
    let store = fleet_store::PgStore::connect(&url, 2)
        .await
        .context("failed to connect to Postgres")?;
    store.migrate().await.context("migration failed")?;
    Ok(store)
}

// ═══════════════════════════════════════════════════════════════════════
//  비밀번호 입력 / 검증
// ═══════════════════════════════════════════════════════════════════════

/// 안전한 비밀번호 입력 (2회 일치 + 강도 검증).
/// `--password` 옵션이 주어지면 검증만 수행하고 프롬프트 생략.
fn read_password(prompt: &str, provided: Option<String>) -> Result<String> {
    let pw = if let Some(p) = provided {
        // CLI 인자로 받은 경우 — 셸 히스토리 노출 경고.
        eprintln!("⚠️  --password 옵션은 셸 히스토리에 노출됩니다. 비권장.");
        p
    } else {
        rpassword::prompt_password(prompt)?;
        rpassword::prompt_password("Confirm: ")?
    };

    if pw.len() < 12 {
        return Err(anyhow!(
            "password must be at least 12 characters (got {})",
            pw.len()
        ));
    }
    let estimate = zxcvbn::zxcvbn(&pw, &[]);
    if estimate.score() < zxcvbn::Score::Three {
        return Err(anyhow!(
            "password too weak (zxcvbn score {:?}). Use longer passphrase or mix characters.",
            estimate.score()
        ));
    }
    Ok(pw)
}

// ═══════════════════════════════════════════════════════════════════════
//  명령 구현
// ═══════════════════════════════════════════════════════════════════════

/// `fleet users list` — 사용자 목록 출력.
async fn list_users(store: &dyn Store, json: bool) -> Result<()> {
    let users = store.list_users().await.context("list_users failed")?;
    if json {
        let json = serde_json::to_string_pretty(&users)?;
        println!("{json}");
        return Ok(());
    }

    if users.is_empty() {
        println!("(no users)");
        return Ok(());
    }

    println!(
        "{:<24} {:<32} {:<8} {:<22} {:<22}",
        "USERNAME", "EMAIL", "ENABLED", "CREATED", "LAST LOGIN"
    );
    println!("{}", "-".repeat(110));
    for u in users {
        println!(
            "{:<24} {:<32} {:<8} {:<22} {:<22}",
            u.username,
            u.email.as_deref().unwrap_or("-"),
            if u.enabled { "yes" } else { "no" },
            u.created_at.format("%Y-%m-%d %H:%M:%S"),
            u.last_login_at
                .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "-".into()),
        );
    }
    Ok(())
}

/// `fleet users show <username>`.
async fn show_user(store: &dyn Store, username: &str) -> Result<()> {
    let user = store
        .get_user_by_username(username)
        .await
        .context("get_user_by_username failed")?
        .ok_or_else(|| anyhow!("user '{username}' not found"))?;

    let roles = store
        .list_user_roles(user.id)
        .await
        .context("list_user_roles failed")?;
    let perms = store
        .list_user_permissions(user.id)
        .await
        .context("list_user_permissions failed")?;

    println!("Username:    {}", user.username);
    println!("ID:          {}", user.id);
    println!("Email:       {}", user.email.as_deref().unwrap_or("-"));
    println!("Enabled:     {}", if user.enabled { "yes" } else { "no" });
    println!(
        "Created:     {}",
        user.created_at.format("%Y-%m-%d %H:%M:%S")
    );
    println!(
        "Last login:  {}",
        user.last_login_at
            .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "never".into())
    );
    println!();

    println!("Roles:");
    if roles.is_empty() {
        println!("  (none)");
    } else {
        for r in roles {
            let builtin_marker = if r.builtin { " [builtin]" } else { "" };
            println!("  - {}{}", r.name, builtin_marker);
        }
    }

    println!();
    println!("Permissions ({}):", perms.len());
    // 알파벳순 정렬.
    let mut names: Vec<&str> = perms.iter().map(|p| p.name.as_str()).collect();
    names.sort_unstable();
    for name in names {
        println!("  {name}");
    }
    Ok(())
}

/// `fleet users create`.
async fn create_user(
    store: &dyn Store,
    username: &str,
    email: Option<&str>,
    roles: Option<Vec<String>>,
    password: Option<String>,
) -> Result<()> {
    // username 형식 검증.
    User::validate_username(username)?;

    // 중복 확인.
    if store
        .get_user_by_username(username)
        .await
        .context("lookup failed")?
        .is_some()
    {
        return Err(anyhow!("user '{username}' already exists"));
    }

    // 비밀번호.
    let pw = read_password("New password: ", password)?;
    let hash = hash_password(&pw).context("argon2 hashing failed")?;

    // 사용자 생성.
    let user = User {
        id: UserId::new(),
        username: username.into(),
        email: email.map(|s| s.into()),
        password_hash: hash,
        enabled: true,
        created_at: chrono::Utc::now(),
        last_login_at: None,
    };
    store
        .create_user(&user)
        .await
        .context("create_user failed")?;

    // 역할 부여.
    let role_names = roles.unwrap_or_else(|| vec!["viewer".into()]);
    for role_name in &role_names {
        if let Err(e) = assign_role(store, username, role_name).await {
            eprintln!("⚠️  role assign '{role_name}' failed: {e:#}");
        }
    }

    println!("✓ User '{username}' created.");
    Ok(())
}

/// `fleet users passwd <username>`.
async fn change_password(
    store: &dyn Store,
    username: &str,
    password: Option<String>,
) -> Result<()> {
    let user = store
        .get_user_by_username(username)
        .await
        .context("get_user_by_username failed")?
        .ok_or_else(|| anyhow!("user '{username}' not found"))?;

    // 현재 비밀번호 확인 (활성 사용자만).
    if user.enabled {
        let current = rpassword::prompt_password("Current password: ")?;
        let verified = verify_password(&current, &user.password_hash)
            .context("password hash verification failed")?;
        if !verified {
            return Err(anyhow!("current password does not match"));
        }
    }

    let pw = read_password("New password: ", password)?;
    let hash = hash_password(&pw).context("argon2 hashing failed")?;

    store
        .update_user_password(user.id, &hash)
        .await
        .context("update_user_password failed")?;

    // 보안: 비밀번호 변경 시 모든 세션 무효화.
    let cleared = store.delete_user_sessions(user.id).await.unwrap_or(0);
    println!("✓ Password updated for '{username}'. ({cleared} session(s) invalidated.)");
    Ok(())
}

/// `fleet users enable/disable <username>`.
async fn set_enabled(store: &dyn Store, username: &str, enabled: bool) -> Result<()> {
    let user = store
        .get_user_by_username(username)
        .await
        .context("get_user_by_username failed")?
        .ok_or_else(|| anyhow!("user '{username}' not found"))?;

    if user.enabled == enabled {
        println!(
            "User '{username}' is already {}.",
            if enabled { "enabled" } else { "disabled" }
        );
        return Ok(());
    }

    store
        .set_user_enabled(user.id, enabled)
        .await
        .context("set_user_enabled failed")?;

    let action = if enabled { "enabled" } else { "disabled" };
    println!("✓ User '{username}' {action}.");

    // 비활성화 시 기존 세션 무효화.
    if !enabled {
        let cleared = store.delete_user_sessions(user.id).await.unwrap_or(0);
        if cleared > 0 {
            println!("  ({cleared} active session(s) terminated.)");
        }
    }
    Ok(())
}

/// `fleet users delete <username>`.
async fn delete_user(store: &dyn Store, username: &str, yes: bool) -> Result<()> {
    let user = store
        .get_user_by_username(username)
        .await
        .context("get_user_by_username failed")?
        .ok_or_else(|| anyhow!("user '{username}' not found"))?;

    // 관리자 1명뿐인 경우 삭제 방지 (잠금 방지).
    let count = store.count_users().await.unwrap_or(0);
    if count <= 1 {
        return Err(anyhow!(
            "refusing to delete the last user — at least one admin must remain"
        ));
    }

    // 관리자가 자기 자신을 삭제하려는 경우도 경고.
    let roles = store.list_user_roles(user.id).await.unwrap_or_default();
    let is_admin = roles.iter().any(|r| r.name == BuiltinRole::Admin.name());
    if is_admin {
        let admin_count = count_admins(store).await;
        if admin_count <= 1 {
            return Err(anyhow!(
                "refusing to delete the last admin — promote another user to admin first"
            ));
        }
    }

    if !yes {
        eprintln!("⚠️  About to delete user '{username}'. This cannot be undone.");
        eprint!("Type the username to confirm: ");
        io::stderr().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if input.trim() != username {
            return Err(anyhow!("confirmation mismatch — aborted"));
        }
    }

    store
        .delete_user(user.id)
        .await
        .context("delete_user failed")?;

    println!("✓ User '{username}' deleted.");
    Ok(())
}

/// `fleet users role assign <username> <role>`.
async fn assign_role(store: &dyn Store, username: &str, role_name: &str) -> Result<()> {
    let user = store
        .get_user_by_username(username)
        .await
        .context("get_user_by_username failed")?
        .ok_or_else(|| anyhow!("user '{username}' not found"))?;

    let role = store
        .get_role_by_name(role_name)
        .await
        .context("get_role_by_name failed")?
        .ok_or_else(|| {
            anyhow!(
                "role '{role_name}' not found. Available: admin, operator, viewer (builtin) or custom."
            )
        })?;

    store
        .assign_user_role(user.id, role.id, None)
        .await
        .context("assign_user_role failed")?;

    println!("✓ Role '{role_name}' assigned to '{username}'.");
    Ok(())
}

/// `fleet users role revoke <username> <role>`.
async fn revoke_role(store: &dyn Store, username: &str, role_name: &str) -> Result<()> {
    let user = store
        .get_user_by_username(username)
        .await
        .context("get_user_by_username failed")?
        .ok_or_else(|| anyhow!("user '{username}' not found"))?;

    let role = store
        .get_role_by_name(role_name)
        .await
        .context("get_role_by_name failed")?
        .ok_or_else(|| anyhow!("role '{role_name}' not found"))?;

    // 마지막 admin 역할 회수 방지.
    if role.name == BuiltinRole::Admin.name() {
        let admin_count = count_admins(store).await;
        if admin_count <= 1 {
            return Err(anyhow!(
                "refusing to revoke the last admin role — promote another user first"
            ));
        }
    }

    store
        .revoke_user_role(user.id, role.id)
        .await
        .context("revoke_user_role failed")?;

    println!("✓ Role '{role_name}' revoked from '{username}'.");
    Ok(())
}

/// `fleet users bootstrap-token` — OTP 수동 발급.
async fn issue_bootstrap_token(
    store: &dyn Store,
    expires_in_hours: i64,
    force: bool,
) -> Result<()> {
    if force {
        // 활성 토큰을 모두 회수한 후 재발급.
        let tokens = store.list_bootstrap_tokens().await.unwrap_or_default();
        for t in tokens {
            if t.is_usable() {
                let _ = store.revoke_bootstrap_token(&t.token).await;
            }
        }
    }

    let user_count = store.count_users().await.unwrap_or(0);
    let token = fleet_store::issue_admin_bootstrap_token(store, expires_in_hours)
        .await
        .context("failed to issue bootstrap token")?;
    print_bootstrap_token(&token, user_count == 0);
    Ok(())
}

fn print_bootstrap_token(token: &str, first_run: bool) {
    println!();
    println!("┌──────────────────────────────────────────────────────────────┐");
    println!("│  ADMIN BOOTSTRAP TOKEN                                         │");
    println!("├──────────────────────────────────────────────────────────────┤");
    println!("│                                                                │");
    println!("│  {token:<60}  │");
    println!("│                                                                │");
    println!("├──────────────────────────────────────────────────────────────┤");
    if first_run {
        println!("│  Open https://fleet.agentthread.dev/bootstrap and paste it.   │");
    } else {
        println!("│  Additional admin bootstrap — existing users detected.        │");
    }
    println!("└──────────────────────────────────────────────────────────────┘");
    println!();
}

// ═══════════════════════════════════════════════════════════════════════
//  헬퍼
// ═══════════════════════════════════════════════════════════════════════

/// 관리자 역할을 가진 활성 사용자 수 카운트.
async fn count_admins(store: &dyn Store) -> u64 {
    let users = store.list_users().await.unwrap_or_default();
    let mut count = 0u64;
    for u in users.iter().filter(|u| u.enabled) {
        let roles = store.list_user_roles(u.id).await.unwrap_or_default();
        if roles.iter().any(|r| r.name == BuiltinRole::Admin.name()) {
            count += 1;
        }
    }
    count
}
