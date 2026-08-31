//! Integration tests: run the `ptj` binary against the fixture archive in
//! `tests/fixtures/archive/`, which mirrors the real layout (one directory
//! per project, a README without frontmatter, a stray `.R` attachment, a
//! hidden directory, and one file exercising the code-fence/setext cases).

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/archive")
}

/// A `ptj` command pinned to the fixture archive and isolated from the
/// invoking user's environment.
fn ptj() -> Command {
    let mut cmd = Command::cargo_bin("ptj").unwrap();
    cmd.env("PROMPTJAR_ROOT", fixture_root())
        .env_remove("PROMPTJAR_MODEL");
    cmd
}

/// A `ptj` command rooted at an arbitrary directory.
fn ptj_at(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("ptj").unwrap();
    cmd.env("PROMPTJAR_ROOT", root)
        .env_remove("PROMPTJAR_MODEL");
    cmd
}

const LIST_TSV: &str = "\
2026-06-30\trevdeprun\trevdeprun/prompts.md\tClaude Fable 5 Extra\t1\t5
2026-07-21\tbisectrunk\tbisectrunk/blog.md\tGPT-5.6 Sol Pro\t2\t58
2026-08-08\tokr\tokr/lockfile.md\tClaude Fable 5 Extra, GPT-5.6 Sol Pro\t2\t12
2026-08-11\tokr\tokr/prompts.md\tClaude Fable 5 Extra\t3\t20
";

#[test]
fn list_prints_one_tsv_row_per_thread() {
    // Hidden dirs (.hidden/) and stray non-Markdown files never appear.
    ptj()
        .arg("list")
        .assert()
        .success()
        .stdout(LIST_TSV)
        .stderr("");
}

#[test]
fn list_records_with_model_filter() {
    ptj()
        .args(["list", "--records", "--model", "gpt"])
        .assert()
        .success()
        .stdout(
            "\
2026-07-21\tbisectrunk\tbisectrunk/blog.md\t1\tGPT-5.6 Sol Pro\t50
2026-07-21\tbisectrunk\tbisectrunk/blog.md\t2\tGPT-5.6 Sol Pro\t8
2026-08-08\tokr\tokr/lockfile.md\t1\tClaude Fable 5 Extra, GPT-5.6 Sol Pro\t6
2026-08-08\tokr\tokr/lockfile.md\t2\tClaude Fable 5 Extra, GPT-5.6 Sol Pro\t6
",
        );
}

#[test]
fn list_project_and_date_filters_are_inclusive() {
    ptj()
        .args([
            "list",
            "--project",
            "okr",
            "--since",
            "2026-08-09",
            "--until",
            "2026-08-11",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("okr/prompts.md"))
        .stdout(predicate::str::contains("lockfile").not());
}

#[test]
fn list_json_rows_parse() {
    let out = ptj().args(["list", "--json"]).output().unwrap();
    let lines: Vec<serde_json::Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[2]["file"], "okr/lockfile.md");
    assert_eq!(
        lines[2]["models"],
        serde_json::json!(["Claude Fable 5 Extra", "GPT-5.6 Sol Pro"])
    );
    assert_eq!(lines[2]["n_records"], 2);
}

#[test]
fn show_prints_a_single_record() {
    ptj()
        .args(["show", "bisectrunk/blog.md:2"])
        .assert()
        .success()
        .stdout("Second prompt: summarize the post in one paragraph.\n");
}

#[test]
fn show_prints_the_whole_body_without_frontmatter() {
    ptj()
        .args(["show", "bisectrunk/blog.md"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("Outline a blog post"))
        .stdout(predicate::str::contains("```yaml"))
        .stdout(predicate::str::contains("date: 2026-07-21"))
        .stdout(predicate::str::ends_with("one paragraph.\n"));
}

#[test]
fn show_rejects_bad_addresses() {
    ptj()
        .args(["show", "okr/prompts.md:9"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no record 9"));
    ptj()
        .args(["show", "okr/nothing.md"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no such thread"));
    ptj()
        .args(["show", "README.md"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not a thread"));
}

#[test]
fn stats_count_multi_model_threads_once_per_model() {
    ptj()
        .arg("stats")
        .assert()
        .success()
        .stdout("Claude Fable 5 Extra\t3\nGPT-5.6 Sol Pro\t2\n");
}

#[test]
fn stats_by_month_with_records_unit() {
    ptj()
        .args(["stats", "--by", "month", "--records"])
        .assert()
        .success()
        .stdout("2026-06\t1\n2026-07\t2\n2026-08\t5\n");
}

#[test]
fn stats_by_project() {
    ptj()
        .args(["stats", "--by", "project"])
        .assert()
        .success()
        .stdout("bisectrunk\t1\nokr\t2\nrevdeprun\t1\n");
}

#[test]
fn lint_warns_without_failing() {
    ptj()
        .arg("lint")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "README.md:1: warning: no YAML frontmatter",
        ))
        .stdout(predicate::str::contains("error").not());
}

#[test]
fn lint_strict_reports_unknown_keys_and_fails_on_warnings() {
    ptj()
        .args(["lint", "--strict"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "lockfile.md:4: warning: unknown frontmatter key `tags`",
        ));
}

#[test]
fn lint_explicit_paths_ignore_non_markdown() {
    let root = fixture_root();
    ptj()
        .arg("lint")
        .arg(root.join("okr/notes.R"))
        .arg(root.join("okr/prompts.md"))
        .assert()
        .success()
        .stdout("");
}

#[test]
fn lint_flags_broken_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("displaced.md"),
        "\n---\ndate: 2026-08-11\nmodel: M\n---\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("baddate.md"),
        "---\ndate: 2026-8-1\nmodel: M\n---\n",
    )
    .unwrap();
    ptj_at(dir.path())
        .arg("lint")
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "baddate.md:2: error: invalid `date`",
        ))
        .stdout(predicate::str::contains(
            "displaced.md:2: error: frontmatter must start at byte 0",
        ));
}

#[test]
fn export_dumps_every_record_with_metadata() {
    let out = ptj().arg("export").output().unwrap();
    let lines: Vec<serde_json::Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 8);
    let lockfile1 = lines
        .iter()
        .find(|l| l["address"] == "okr/lockfile.md:1")
        .expect("lockfile record 1");
    assert_eq!(
        lockfile1["extra"]["tags"],
        serde_json::json!(["lockfile", "ci"])
    );
    assert_eq!(lockfile1["text"], "Explain the lock file drift check.");
    assert_eq!(lockfile1["date"], "2026-08-08");
}

#[test]
fn new_creates_a_thread_and_refuses_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    ptj_at(dir.path())
        .args([
            "new",
            "okr",
            "--model",
            "Claude Fable 5 Extra",
            "--date",
            "2026-08-30",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("okr/prompts.md"));
    let written = std::fs::read_to_string(dir.path().join("okr/prompts.md")).unwrap();
    assert_eq!(
        written,
        "---\ndate: 2026-08-30\nmodel: Claude Fable 5 Extra\n---\n\n"
    );
    ptj_at(dir.path())
        .args(["new", "okr", "--model", "M"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("refusing to overwrite"));
}

#[test]
fn new_writes_multiple_models_as_a_flow_sequence() {
    let dir = tempfile::tempdir().unwrap();
    ptj_at(dir.path())
        .args([
            "new",
            "okr",
            "blog.md",
            "--model",
            "A",
            "--model",
            "B",
            "--date",
            "2026-08-30",
        ])
        .assert()
        .success();
    let written = std::fs::read_to_string(dir.path().join("okr/blog.md")).unwrap();
    assert_eq!(written, "---\ndate: 2026-08-30\nmodel: [A, B]\n---\n\n");
}

#[test]
fn new_falls_back_to_the_model_env_var() {
    let dir = tempfile::tempdir().unwrap();
    ptj_at(dir.path())
        .env("PROMPTJAR_MODEL", "GPT-5.6 Sol Pro")
        .args(["new", "revdeprun", "--date", "2026-08-30"])
        .assert()
        .success();
    let written = std::fs::read_to_string(dir.path().join("revdeprun/prompts.md")).unwrap();
    assert!(written.contains("model: GPT-5.6 Sol Pro"), "{written}");
}

#[test]
fn new_without_any_model_fails() {
    let dir = tempfile::tempdir().unwrap();
    ptj_at(dir.path())
        .args(["new", "okr"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no model given"));
}

#[test]
fn root_discovery_walks_up_to_the_git_root() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    std::fs::create_dir(dir.path().join("proj")).unwrap();
    std::fs::write(
        dir.path().join("proj/a.md"),
        "---\ndate: 2026-08-30\nmodel: M\n---\n\nHello.\n",
    )
    .unwrap();
    let mut cmd = Command::cargo_bin("ptj").unwrap();
    cmd.env_remove("PROMPTJAR_ROOT")
        .env_remove("PROMPTJAR_MODEL")
        .current_dir(dir.path().join("proj"))
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("proj/a.md"));
}

#[test]
fn non_utf8_markdown_is_skipped_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("bad.md"), [0xffu8, 0xfe, 0x00, 0x01]).unwrap();
    std::fs::write(
        dir.path().join("ok.md"),
        "---\ndate: 2026-08-30\nmodel: M\n---\n\nHi.\n",
    )
    .unwrap();
    let out = ptj_at(dir.path()).arg("list").output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap().lines().count(), 1);
    assert!(String::from_utf8_lossy(&out.stderr).contains("non-UTF-8"));
}

#[test]
fn invalid_threads_are_skipped_with_a_warning_not_silently() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("broken.md"),
        "---\ndate: nope\nmodel: M\n---\n\nx\n",
    )
    .unwrap();
    let out = ptj_at(dir.path()).arg("list").output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("broken.md"));
}
