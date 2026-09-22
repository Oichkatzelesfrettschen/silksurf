use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct Corpus(PathBuf);

impl Corpus {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "silksurf-test262-admission-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(path.join("test/language")).unwrap();
        std::fs::create_dir(path.join("harness")).unwrap();
        Self(path)
    }

    fn run(&self, scorecard: &std::path::Path) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_test262_boa"))
            .arg("--dir")
            .arg(self.0.join("test/language"))
            .arg("--scorecard")
            .arg(scorecard)
            .output()
            .expect("runner starts")
    }
}

impl Drop for Corpus {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove owned test corpus");
    }
}

#[test]
fn runner_rejects_invalid_worker_counts_before_scanning() {
    let corpus = Corpus::new();
    for arguments in [
        vec!["--jobs", "0"],
        vec!["--jobs", "invalid"],
        vec!["--jobs"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_test262_boa"))
            .arg("--dir")
            .arg(corpus.0.join("test/language"))
            .args(arguments)
            .output()
            .expect("runner starts");
        assert_eq!(output.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("--jobs requires a positive integer")
        );
    }
}

#[test]
fn runner_accounts_for_failure_skip_read_error_empty_selection_and_write_error() {
    let corpus = Corpus::new();
    let scorecard = corpus.0.join("scorecard.json");
    assert!(!corpus.run(&scorecard).status.success(), "empty selection");
    let test = corpus.0.join("test/language/case.js");
    std::fs::write(&test, "/*---\nflags: [raw]\n---*/\n1 + 1;").unwrap();
    let passed = corpus.run(&scorecard);
    assert!(
        passed.status.success(),
        "{}",
        String::from_utf8_lossy(&passed.stdout)
    );
    std::fs::write(
        &test,
        "/*---\nflags: [raw]\n---*/\nthrow new Error('failure');",
    )
    .unwrap();
    assert!(!corpus.run(&scorecard).status.success(), "failed oracle");
    std::fs::write(&test, "/*---\nfeatures: [Temporal]\n---*/\n1;").unwrap();
    assert!(
        !corpus.run(&scorecard).status.success(),
        "skip rejects admission"
    );
    std::fs::write(&test, [0xff]).unwrap();
    assert!(
        !corpus.run(&scorecard).status.success(),
        "source decoding fails"
    );
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&scorecard).unwrap()).unwrap();
    assert_eq!(summary["total"], 1);
    assert_eq!(summary["fail"], 1);
    std::fs::write(&test, "/*---\nflags: [raw]\n---*/\n1;").unwrap();
    assert!(
        !corpus.run(&corpus.0).status.success(),
        "scorecard write fails"
    );
}
