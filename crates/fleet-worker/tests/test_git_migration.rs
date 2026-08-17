use std::fs::{self, File};
use std::io::Write;
use std::process::Command;
use tempfile::TempDir;

fn run_git(dir: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("failed to execute git command");
    if !output.status.success() {
        panic!(
            "git command failed: {:?} in {:?}\nstdout: {}\nstderr: {}",
            args,
            dir,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn test_e2e_git_migration_flow() {
    // 1. 임시 디렉토리 생성
    let temp_dir = TempDir::new().unwrap();
    let root_path = temp_dir.path();

    let remote_dir = root_path.join("remote.git");
    let worker_a_dir = root_path.join("worker_a");
    let worker_b_dir = root_path.join("worker_b");

    fs::create_dir_all(&remote_dir).unwrap();
    fs::create_dir_all(&worker_a_dir).unwrap();
    fs::create_dir_all(&worker_b_dir).unwrap();

    // 2. 원격 Bare 저장소 생성 (Gitea 역할)
    run_git(&remote_dir, &["init", "--bare", "--initial-branch=main"]);

    // 3. Worker A 초기화 및 최초 커밋 (main 브랜치 생성 목적)
    run_git(&worker_a_dir, &["init", "--initial-branch=main"]);
    run_git(&worker_a_dir, &["config", "user.name", "Test User"]);
    run_git(&worker_a_dir, &["config", "user.email", "test@example.com"]);
    run_git(&worker_a_dir, &["remote", "add", "origin", remote_dir.to_str().unwrap()]);

    let readme_path = worker_a_dir.join("README.md");
    let mut f = File::create(&readme_path).unwrap();
    writeln!(f, "# Grok Fleet Project").unwrap();
    drop(f);

    run_git(&worker_a_dir, &["add", "README.md"]);
    run_git(&worker_a_dir, &["commit", "-m", "initial commit"]);
    run_git(&worker_a_dir, &["push", "-u", "origin", "main"]);

    // 4. Worker B 초기화 및 clone/fetch 설정
    run_git(&worker_b_dir, &["init", "--initial-branch=main"]);
    run_git(&worker_b_dir, &["config", "user.name", "Test User"]);
    run_git(&worker_b_dir, &["config", "user.email", "test@example.com"]);
    run_git(&worker_b_dir, &["remote", "add", "origin", remote_dir.to_str().unwrap()]);
    run_git(&worker_b_dir, &["fetch", "origin"]);
    run_git(&worker_b_dir, &["checkout", "main"]);

    // 5. Worker A에서 태스크 수행 중 체크포인트 생성 (Draining 발생 상황 모사)
    // 새로운 파일 및 하위 폴더 생성
    let code_dir = worker_a_dir.join("src");
    fs::create_dir_all(&code_dir).unwrap();
    let main_rs = code_dir.join("main.rs");
    let mut f2 = File::create(&main_rs).unwrap();
    writeln!(f2, "fn main() {{ println!(\"hello from worker a\"); }}").unwrap();
    drop(f2);

    // git add . & commit & push
    let task_id = "task-1234-uuid-abcd";
    let branch_name = format!("tmp/task-{}", task_id);

    run_git(&worker_a_dir, &["checkout", "-b", &branch_name]);
    run_git(&worker_a_dir, &["add", "."]);
    run_git(&worker_a_dir, &["commit", "-m", "checkpoint: task-1234 migration"]);
    run_git(&worker_a_dir, &["push", "origin", &branch_name]);

    // 6. Worker B에서 해당 태스크를 수령하여 workspace 이관 & 복구 진행
    // fetch, checkout, reset --hard, clean -fdx
    run_git(&worker_b_dir, &["fetch", "origin"]);
    run_git(&worker_b_dir, &["checkout", "-B", &branch_name, &format!("origin/{}", branch_name)]);
    run_git(&worker_b_dir, &["reset", "--hard", &format!("origin/{}", branch_name)]);
    run_git(&worker_b_dir, &["clean", "-fdx"]);

    // 7. Worker B의 이관 복구 결과 검증
    let main_rs_b = worker_b_dir.join("src/main.rs");
    assert!(main_rs_b.exists(), "src/main.rs must exist in Worker B after migration");
    let content = fs::read_to_string(main_rs_b).unwrap();
    assert!(content.contains("hello from worker a"));
}
