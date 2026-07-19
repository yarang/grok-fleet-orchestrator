//! `fleet doctor` — 인프라 진단.
//!
//! 데이터베이스 연결, 마이그레이션 상태, 워커 가용성, (옵션) HTTP API와
//! 대시보드의 헬스를 점검하고 사람이 읽기 쉬운 보고서를 출력합니다.
//!
//! 종료 코드:
//! - 모든 검사 OK → 0
//! - 경고(WARN)만 있는 경우 → 0 (정상으로 간주)
//! - 실패(FAIL)가 하나라도 있는 경우 → 1

use std::time::Duration;

use anyhow::Result;

use fleet_core::{TaskFilter, WorkerFilter, WorkerStatus};
use fleet_store::{PgStore, Store};

/// `doctor` 명령 진입.
pub async fn run_doctor(
    api_url: Option<String>,
    dashboard_url: Option<String>,
    db_max_conn: u32,
) -> Result<()> {
    let mut report = Report::default();

    // 1. DATABASE_URL 환경변수
    let db_url = match std::env::var("DATABASE_URL") {
        Ok(u) => {
            report.ok("DATABASE_URL", "environment variable is set");
            u
        }
        Err(_) => {
            report.fail(
                "DATABASE_URL",
                "environment variable is NOT set (export DATABASE_URL=postgres://...)",
            );
            report.print();
            return Err(anyhow::anyhow!("DATABASE_URL missing — aborting doctor"));
        }
    };

    // 2. Postgres 연결
    let store = match PgStore::connect(&db_url, db_max_conn).await {
        Ok(s) => {
            report.ok("postgres_connect", "connected to Postgres");
            s
        }
        Err(e) => {
            report.fail("postgres_connect", &format!("failed to connect: {e}"));
            report.print();
            return Err(anyhow::anyhow!("Postgres connection failed"));
        }
    };

    // 3. 마이그레이션
    match store.migrate().await {
        Ok(()) => report.ok("migrations", "applied successfully"),
        Err(e) => report.warn(
            "migrations",
            &format!("migration check failed (may need `fleet migrate`): {e}"),
        ),
    }

    // 4. 워커 요약
    match store.list_workers(&WorkerFilter::default()).await {
        Ok(workers) => {
            let online = workers
                .iter()
                .filter(|w| matches!(w.status, WorkerStatus::Online))
                .count();
            let offline = workers
                .iter()
                .filter(|w| matches!(w.status, WorkerStatus::Offline))
                .count();
            report.ok(
                "workers",
                &format!(
                    "{} total (online={}, offline={})",
                    workers.len(),
                    online,
                    offline
                ),
            );
            if !workers.is_empty() && online == 0 {
                report.warn(
                    "dispatch_readiness",
                    "no online workers — new tasks will stay pending",
                );
            }
        }
        Err(e) => {
            report.warn("workers", &format!("failed to list: {e}"));
        }
    }

    // 5. 작업 백로그
    match store
        .list_tasks(&TaskFilter {
            limit: 1,
            ..Default::default()
        })
        .await
    {
        Ok(tasks) => {
            report.ok(
                "tasks",
                &format!("backend reachable (sampled {} task(s))", tasks.len()),
            );
        }
        Err(e) => {
            report.warn("tasks", &format!("failed to list: {e}"));
        }
    }

    // 6. HTTP API 헬스 (옵션)
    if let Some(url) = api_url {
        let endpoint = format!("{}/v1/health", url.trim_end_matches('/'));
        let result = tokio::time::timeout(Duration::from_secs(5), reqwest::get(&endpoint)).await;
        match result {
            Ok(Ok(resp)) if resp.status().is_success() => {
                report.ok(
                    "api_health",
                    &format!("{endpoint} returned {}", resp.status()),
                );
            }
            Ok(Ok(resp)) => {
                report.warn(
                    "api_health",
                    &format!("{endpoint} returned non-success status {}", resp.status()),
                );
            }
            Ok(Err(e)) => {
                report.warn("api_health", &format!("request failed: {e}"));
            }
            Err(_) => {
                report.warn("api_health", "timed out after 5s");
            }
        }
    }

    // 7. 대시보드 헬스 (옵션)
    if let Some(url) = dashboard_url {
        let endpoint = format!("{}/health", url.trim_end_matches('/'));
        let result = tokio::time::timeout(Duration::from_secs(5), reqwest::get(&endpoint)).await;
        match result {
            Ok(Ok(resp)) if resp.status().is_success() => {
                report.ok(
                    "dashboard_health",
                    &format!("{endpoint} returned {}", resp.status()),
                );
            }
            Ok(Ok(resp)) => {
                report.warn(
                    "dashboard_health",
                    &format!("{endpoint} returned non-success status {}", resp.status()),
                );
            }
            Ok(Err(e)) => {
                report.warn("dashboard_health", &format!("request failed: {e}"));
            }
            Err(_) => {
                report.warn("dashboard_health", "timed out after 5s");
            }
        }
    }

    report.print();
    if report.has_failures() {
        Err(anyhow::anyhow!("one or more checks FAILED"))
    } else {
        Ok(())
    }
}

// ── Report helpers ───────────────────────────────────────────────────

#[derive(Default)]
struct Report {
    rows: Vec<Row>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Ok,
    Warn,
    Fail,
}

struct Row {
    name: String,
    status: Status,
    message: String,
}

impl Report {
    fn ok(&mut self, name: &str, msg: &str) {
        self.rows.push(Row {
            name: name.to_string(),
            status: Status::Ok,
            message: msg.to_string(),
        });
    }

    fn warn(&mut self, name: &str, msg: &str) {
        self.rows.push(Row {
            name: name.to_string(),
            status: Status::Warn,
            message: msg.to_string(),
        });
    }

    fn fail(&mut self, name: &str, msg: &str) {
        self.rows.push(Row {
            name: name.to_string(),
            status: Status::Fail,
            message: msg.to_string(),
        });
    }

    fn has_failures(&self) -> bool {
        self.rows.iter().any(|r| r.status == Status::Fail)
    }

    fn print(&self) {
        println!("\n{}", "=".repeat(78));
        println!("{:<30} {:<8} DETAIL", "CHECK", "STATUS");
        println!("{}", "=".repeat(78));
        for r in &self.rows {
            let mark = match r.status {
                Status::Ok => "OK",
                Status::Warn => "WARN",
                Status::Fail => "FAIL",
            };
            println!("{:<30} {:<8} {}", r.name, mark, r.message);
        }
        println!("{}", "=".repeat(78));
        let ok = self.rows.iter().filter(|r| r.status == Status::Ok).count();
        let warn = self
            .rows
            .iter()
            .filter(|r| r.status == Status::Warn)
            .count();
        let fail = self
            .rows
            .iter()
            .filter(|r| r.status == Status::Fail)
            .count();
        println!(
            "summary: {ok} OK, {warn} WARN, {fail} FAIL (total {})",
            self.rows.len()
        );
        println!("{}", "=".repeat(78));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_tracking() {
        let mut r = Report::default();
        r.ok("a", "good");
        r.warn("b", "watch");
        r.fail("c", "broken");
        assert!(r.has_failures());
        assert_eq!(r.rows.len(), 3);
    }

    #[test]
    fn report_all_ok_no_failures() {
        let mut r = Report::default();
        r.ok("a", "good");
        r.ok("b", "also good");
        assert!(!r.has_failures());
    }
}
