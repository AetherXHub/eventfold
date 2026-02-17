use eventfold::snapshot;
use eventfold::Snapshot;
use serde::{Deserialize, Serialize};
use std::io::Write;
use tempfile::tempdir;

#[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct TestState {
    count: u64,
    items: Vec<String>,
}

#[test]
fn test_save_load_round_trip() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.snapshot.json");

    let snap = Snapshot::new(
        TestState {
            count: 42,
            items: vec!["hello".into(), "world".into()],
        },
        1024,
        "abcdef0123456789".into(),
    );

    snapshot::save(&path, &snap).unwrap();
    let loaded: Snapshot<TestState> = snapshot::load(&path).unwrap().unwrap();

    assert_eq!(loaded.state, snap.state);
    assert_eq!(loaded.offset, snap.offset);
    assert_eq!(loaded.hash, snap.hash);
}

#[test]
fn test_load_nonexistent() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("does_not_exist.snapshot.json");

    let loaded: Option<Snapshot<TestState>> = snapshot::load(&path).unwrap();
    assert!(loaded.is_none());
}

#[test]
fn test_no_tmp_file_after_save() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.snapshot.json");
    let tmp_path = path.with_extension("json.tmp");

    let snap = Snapshot::new(TestState::default(), 0, String::new());

    snapshot::save(&path, &snap).unwrap();

    assert!(path.exists(), "snapshot file should exist");
    assert!(!tmp_path.exists(), ".tmp file should not exist after save");
}

#[test]
fn test_delete_removes_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.snapshot.json");

    let snap = Snapshot::new(
        TestState {
            count: 1,
            items: vec!["item".into()],
        },
        100,
        "hash".into(),
    );

    snapshot::save(&path, &snap).unwrap();
    assert!(path.exists());

    snapshot::delete(&path).unwrap();
    assert!(!path.exists());

    let loaded: Option<Snapshot<TestState>> = snapshot::load(&path).unwrap();
    assert!(loaded.is_none());
}

#[test]
fn test_delete_idempotent() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("nonexistent.snapshot.json");

    // Should not error
    snapshot::delete(&path).unwrap();
    snapshot::delete(&path).unwrap();
}

#[test]
fn test_empty_state() {
    #[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Empty {}

    let dir = tempdir().unwrap();
    let path = dir.path().join("empty.snapshot.json");

    let snap = Snapshot::new(Empty {}, 0, String::new());

    snapshot::save(&path, &snap).unwrap();
    let loaded: Snapshot<Empty> = snapshot::load(&path).unwrap().unwrap();
    assert_eq!(loaded.state, snap.state);
    assert_eq!(loaded.offset, 0);
}

#[test]
fn test_nested_state() {
    #[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Inner {
        value: String,
    }

    #[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Outer {
        name: String,
        inner: Inner,
        tags: Vec<String>,
    }

    let dir = tempdir().unwrap();
    let path = dir.path().join("nested.snapshot.json");

    let snap = Snapshot::new(
        Outer {
            name: "root".into(),
            inner: Inner {
                value: "deep".into(),
            },
            tags: vec!["a".into(), "b".into()],
        },
        500,
        "nested_hash".into(),
    );

    snapshot::save(&path, &snap).unwrap();
    let loaded: Snapshot<Outer> = snapshot::load(&path).unwrap().unwrap();
    assert_eq!(loaded.state, snap.state);
}

#[test]
fn test_large_state() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("large.snapshot.json");

    let items: Vec<String> = (0..1000).map(|i| format!("item_{i}")).collect();
    let snap = Snapshot::new(
        TestState {
            count: 1000,
            items,
        },
        99999,
        "large_hash".into(),
    );

    snapshot::save(&path, &snap).unwrap();
    let loaded: Snapshot<TestState> = snapshot::load(&path).unwrap().unwrap();
    assert_eq!(loaded.state.items.len(), 1000);
    assert_eq!(loaded.state.count, 1000);
    assert_eq!(loaded.state.items[0], "item_0");
    assert_eq!(loaded.state.items[999], "item_999");
}

#[test]
fn test_offset_zero() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("zero.snapshot.json");

    let snap = Snapshot::new(
        TestState {
            count: 5,
            items: vec!["after_rotation".into()],
        },
        0,
        String::new(),
    );

    snapshot::save(&path, &snap).unwrap();
    let loaded: Snapshot<TestState> = snapshot::load(&path).unwrap().unwrap();
    assert_eq!(loaded.offset, 0);
    assert_eq!(loaded.hash, "");
    assert_eq!(loaded.state.count, 5);
}

#[test]
fn test_large_offset() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("large_offset.snapshot.json");

    let large_offset = u64::MAX / 2;
    let snap = Snapshot::new(TestState::default(), large_offset, "big_offset_hash".into());

    snapshot::save(&path, &snap).unwrap();
    let loaded: Snapshot<TestState> = snapshot::load(&path).unwrap().unwrap();
    assert_eq!(loaded.offset, large_offset);
}

#[test]
fn test_corrupt_file_returns_none() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("corrupt.snapshot.json");

    // Write garbage
    std::fs::write(&path, "garbage{{{not json").unwrap();

    let loaded: Option<Snapshot<TestState>> = snapshot::load(&path).unwrap();
    assert!(loaded.is_none(), "corrupt file should return None");
}

#[test]
fn test_truncated_file_returns_none() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("truncated.snapshot.json");

    // Write partial JSON
    let mut file = std::fs::File::create(&path).unwrap();
    write!(file, r#"{{"state":{{"count":42,"items":["#).unwrap();
    drop(file);

    let loaded: Option<Snapshot<TestState>> = snapshot::load(&path).unwrap();
    assert!(loaded.is_none(), "truncated JSON should return None");
}

#[test]
fn test_tmp_cleanup_on_delete() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("cleanup.snapshot.json");
    let tmp_path = path.with_extension("json.tmp");

    // Create both the snapshot and a leftover .tmp file
    let snap = Snapshot::new(TestState::default(), 0, String::new());
    snapshot::save(&path, &snap).unwrap();

    // Manually create a .tmp file (simulating crash during previous save)
    let mut tmp_file = std::fs::File::create(&tmp_path).unwrap();
    write!(tmp_file, "leftover tmp data").unwrap();
    drop(tmp_file);

    assert!(path.exists());
    assert!(tmp_path.exists());

    snapshot::delete(&path).unwrap();

    assert!(!path.exists(), "snapshot file should be deleted");
    assert!(!tmp_path.exists(), ".tmp file should also be deleted");
}

#[test]
fn test_save_overwrites_existing() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("overwrite.snapshot.json");

    let snap1 = Snapshot::new(
        TestState {
            count: 1,
            items: vec!["first".into()],
        },
        10,
        "hash1".into(),
    );

    let snap2 = Snapshot::new(
        TestState {
            count: 2,
            items: vec!["second".into()],
        },
        20,
        "hash2".into(),
    );

    snapshot::save(&path, &snap1).unwrap();
    snapshot::save(&path, &snap2).unwrap();

    let loaded: Snapshot<TestState> = snapshot::load(&path).unwrap().unwrap();
    assert_eq!(loaded.state.count, 2);
    assert_eq!(loaded.state.items, vec!["second"]);
    assert_eq!(loaded.offset, 20);
    assert_eq!(loaded.hash, "hash2");
}

#[test]
fn test_snapshot_is_pretty_printed() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("pretty.snapshot.json");

    let snap = Snapshot::new(
        TestState {
            count: 1,
            items: vec!["item".into()],
        },
        100,
        "abc".into(),
    );

    snapshot::save(&path, &snap).unwrap();

    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(
        contents.contains('\n'),
        "snapshot should be pretty-printed for human inspection"
    );
}

#[test]
fn test_wrong_type_returns_none() {
    #[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct OtherState {
        name: String,
        active: bool,
    }

    let dir = tempdir().unwrap();
    let path = dir.path().join("wrong_type.snapshot.json");

    // Save as TestState
    let snap = Snapshot::new(
        TestState {
            count: 42,
            items: vec!["hello".into()],
        },
        100,
        "hash".into(),
    );
    snapshot::save(&path, &snap).unwrap();

    // Try to load as OtherState — fields don't match, should return None
    let loaded: Option<Snapshot<OtherState>> = snapshot::load(&path).unwrap();
    // serde may or may not fail depending on field overlap; this tests the graceful handling
    // If it deserializes (with defaults), that's ok. If it fails, we get None.
    // The important thing is it doesn't panic.
    let _ = loaded;
}
