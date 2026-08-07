#!/usr/bin/env python3
import sys
import os
import urllib.request
import json
import time
import argparse

def get_runs(owner, repo, token=None):
    url = f"https://api.github.com/repos/{owner}/{repo}/actions/runs"
    req = urllib.request.Request(url)
    req.add_header("User-Agent", "Antigravity-CI-Monitor")
    req.add_header("Accept", "application/vnd.github.v3+json")
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(req) as res:
            return json.loads(res.read().decode("utf-8"))
    except Exception as e:
        print(f"Error fetching GitHub Actions runs: {e}", file=sys.stderr)
        return None

def monitor(owner, repo, commit_sha, poll_interval=15, max_attempts=20, token=None):
    print(f"Starting GitHub Actions CI monitoring for commit: {commit_sha[:7]} in {owner}/{repo}")
    attempts = 0
    while attempts < max_attempts:
        runs_data = get_runs(owner, repo, token)
        if not runs_data or "workflow_runs" not in runs_data:
            print("Failed to fetch runs. Retrying...")
            time.sleep(poll_interval)
            attempts += 1
            continue

        target_run = None
        for run in runs_data["workflow_runs"]:
            if run["head_sha"] == commit_sha:
                target_run = run
                break

        if not target_run:
            print("No run found matching commit SHA. Waiting for GitHub to trigger...")
            time.sleep(poll_interval)
            attempts += 1
            continue

        status = target_run.get("status")
        conclusion = target_run.get("conclusion")
        run_url = target_run.get("html_url")
        print(f"[{attempts+1}/{max_attempts}] Run ID: {target_run['id']} | Status: {status} | Conclusion: {conclusion}")

        if status == "completed":
            if conclusion == "success":
                print("🟢 GitHub Actions CI PASSED successfully!")
                sys.exit(0)
            else:
                print(f"🔴 GitHub Actions CI FAILED with conclusion: {conclusion}")
                print(f"View details here: {run_url}")
                sys.exit(1)

        time.sleep(poll_interval)
        attempts += 1

    print("⚠️ Timeout: CI monitoring exceeded maximum wait time.")
    sys.exit(2)

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Monitor GitHub Actions CI status for a commit.")
    parser.add_argument("--owner", default="yarang", help="GitHub repo owner")
    parser.add_argument("--repo", default="grok-fleet-orchestrator", help="GitHub repo name")
    parser.add_argument("--commit", required=True, help="Full commit SHA to monitor")
    parser.add_argument("--poll", type=int, default=15, help="Poll interval in seconds")
    parser.add_argument("--max-wait", type=int, default=300, help="Max wait time in seconds")
    args = parser.parse_args()

    token = os.environ.get("GITHUB_TOKEN")
    max_attempts = max(1, args.max_wait // args.poll)
    monitor(args.owner, args.repo, args.commit, poll_interval=args.poll, max_attempts=max_attempts, token=token)
