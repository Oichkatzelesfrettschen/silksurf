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
            "silksurf-wpt-admission-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn run(&self, scorecard: &std::path::Path) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_wpt_runner"))
            .arg("--dir")
            .arg(&self.0)
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
fn runner_rejects_failure_skip_empty_selection_and_scorecard_errors() {
    let corpus = Corpus::new();
    let scorecard = corpus.0.join("scorecard.json");
    assert!(!corpus.run(&scorecard).status.success(), "empty catalog");
    std::fs::write(
        corpus.0.join("html_unordered_list.html"),
        include_str!("../conformance/wpt/fixtures/html_unordered_list.html"),
    )
    .unwrap();
    assert!(corpus.run(&scorecard).status.success(), "valid fixture");
    let corrupted = corpus.0.join("html_ordered_list.html");
    std::fs::write(&corrupted, "<!doctype html><p>missing ordered list").unwrap();
    let failed = corpus.run(&scorecard);
    assert!(
        !failed.status.success(),
        "one failure must reject a 50 percent run"
    );
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&scorecard).unwrap()).unwrap();
    assert_eq!(summary["fail"], 1);
    std::fs::rename(&corrupted, corpus.0.join("unregistered_fixture.html")).unwrap();
    assert!(
        !corpus.run(&scorecard).status.success(),
        "unregistered fixture"
    );
    std::fs::remove_file(corpus.0.join("unregistered_fixture.html")).unwrap();
    assert!(
        !corpus.run(&corpus.0).status.success(),
        "scorecard write fails"
    );
}
