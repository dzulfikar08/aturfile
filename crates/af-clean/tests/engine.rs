//! Engine integration tests: real tempdir fixtures, real walks.

use af_clean::cleanup::{apply, check_protected, CleanupQueue, Disposal, StagedItem};
use af_clean::dupes::find_duplicates;
use af_clean::other::find_dev_junk;
use af_clean::snapshot::{diff, prune_for_export};
use af_clean::{analysis, scan, ScanOptions};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

fn tmpdir(tag: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("dbc-{tag}-"))
        .tempdir()
        .unwrap()
}

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = fs::File::create(path).unwrap();
    f.write_all(bytes).unwrap();
}

fn scan_dir(p: &Path) -> af_clean::ScanResult {
    scan::scan(p, ScanOptions::default(), None).unwrap()
}

#[test]
fn scanner_aggregates_sizes_and_counts() {
    let td = tmpdir("agg");
    let root = td.path();
    write(&root.join("a.txt"), &[0u8; 1000]);
    write(&root.join("sub/b.bin"), &[0u8; 2500]);
    write(&root.join("sub/c.bin"), &[0u8; 3500]);
    fs::create_dir(root.join("empty")).unwrap();

    let r = scan_dir(root);
    assert_eq!(r.node.logical, 7000);
    assert_eq!(r.node.files, 3);
    assert_eq!(r.node.dirs, 2); // sub + empty
    let sub = r.node.children.iter().find(|c| c.name == "sub").unwrap();
    assert_eq!(sub.logical, 6000);
    assert_eq!(sub.files, 2);
    // children sorted biggest-first
    let names: Vec<_> = r.node.children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["sub", "a.txt", "empty"]);
}

#[test]
fn scanner_ignores_symlinks_and_counts_hardlinks_once() {
    let td = tmpdir("links");
    let root = td.path();
    write(&root.join("real.bin"), &[7u8; 4096]);
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("real.bin"), root.join("link.bin")).unwrap();

    let r = scan_dir(root);
    let logicals: Vec<u64> = r.node.children.iter().map(|c| c.logical).collect();
    assert!(logicals.contains(&4096));
    assert!(
        logicals.iter().all(|&l| l <= 4096),
        "symlink must not follow: {logicals:?}"
    );

    // Hardlink: same inode twice — counted once.
    let other = root.join("hard.bin");
    fs::hard_link(root.join("real.bin"), &other).unwrap();
    let r2 = scan_dir(root);
    assert_eq!(r2.node.logical, 4096);
}

#[test]
fn scanner_marks_truncation_at_cap() {
    let td = tmpdir("cap");
    let root = td.path();
    for i in 0..20 {
        write(&root.join(format!("f{i}.txt")), &[0u8; 10]);
    }
    let r = scan::scan(
        root,
        ScanOptions {
            max_files: Some(5),
            ..ScanOptions::default()
        },
        None,
    )
    .unwrap();
    assert!(r.truncated);
    assert!(r.node.files <= 5);
}

#[test]
fn duplicates_found_by_content_not_name() {
    let td = tmpdir("dupes");
    let root = td.path();
    let payload: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    write(&root.join("one.dat"), &payload);
    write(&root.join("two.dat"), &payload); // same bytes, different name
    let mut payload2 = payload.clone();
    payload2[0] ^= 0xff;
    write(&root.join("three.dat"), &payload2); // same size, different bytes

    let r = scan_dir(root);
    let groups = find_duplicates(&r, 1000).unwrap();
    assert_eq!(groups.len(), 1, "three.dat differs in content");
    let g = &groups[0];
    assert_eq!(g.files.len(), 2);
    assert_eq!(g.wasted, payload.len() as u64);
}

#[test]
fn age_and_top_views() {
    let td = tmpdir("age");
    let root = td.path();
    write(&root.join("recent.txt"), &[0u8; 1000]);
    let old = root.join("old.txt");
    write(&old, &[0u8; 8000]);
    let old_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        - 400 * 86_400;
    let f = fs::File::options().write(true).open(&old).unwrap();
    f.set_times(
        std::fs::FileTimes::new()
            .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(old_time as u64)),
    )
    .unwrap();

    let r = scan_dir(root);
    let now = chrono::Utc::now().timestamp();
    let buckets = analysis::age_buckets(&r, now);
    assert_eq!(buckets.last_7d, 1000);
    assert_eq!(buckets.older, 8000);

    let untouched = analysis::big_untouched(&r, 1_000_000, 365, now);
    assert_eq!(untouched.len(), 0, "8KB file sits below the 1MB floor");

    let untouched2 = analysis::big_untouched(&r, 1000, 365, now);
    assert_eq!(untouched2.len(), 1);
    assert_eq!(untouched2[0].path, old.canonicalize().unwrap());

    let top = analysis::top_sizes(&r, 1);
    assert_eq!(top[0].name, "old.txt");
}

#[test]
fn snapshot_diff_reports_growth() {
    let td = tmpdir("snap");
    let root = td.path();
    write(&root.join("grow.txt"), &[0u8; 100]);
    write(&root.join("gone.txt"), &[0u8; 300]);
    let old_scan = scan_dir(root);

    write(&root.join("grow.txt"), &[0u8; 900]);
    write(&root.join("new.txt"), &[0u8; 500]);
    fs::remove_file(root.join("gone.txt")).unwrap();
    let new_scan = scan_dir(root);

    let deltas = diff(&old_scan, &new_scan);
    assert_eq!(deltas.len(), 3);
    let grow = deltas
        .iter()
        .find(|d| d.path.ends_with("grow.txt"))
        .unwrap();
    assert_eq!(grow.delta, 800);
    assert_eq!(grow.old_bytes, 100);
    assert_eq!(grow.new_bytes, 900);
    let newf = deltas.iter().find(|d| d.path.ends_with("new.txt")).unwrap();
    assert_eq!(newf.delta, 500);
    assert_eq!(newf.old_bytes, 0);
    let gone = deltas
        .iter()
        .find(|d| d.path.ends_with("gone.txt"))
        .unwrap();
    assert_eq!(gone.delta, -300);
    assert_eq!(gone.new_bytes, 0);
    // Sorted: biggest gainer first.
    assert_eq!(deltas[0].delta, 800);
}

#[test]
fn prune_export_keeps_sizes_consistent() {
    let td = tmpdir("prune");
    let root = td.path();
    write(&root.join("tiny1.txt"), &[0u8; 10]);
    write(&root.join("tiny2.txt"), &[0u8; 20]);
    write(&root.join("big.txt"), &[0u8; 10_000]);
    let r = scan_dir(root);
    let pruned = prune_for_export(&r.node, 1000, 24);
    let smalls: Vec<_> = pruned
        .children
        .iter()
        .filter(|c| c.name.starts_with("·"))
        .collect();
    assert_eq!(smalls.len(), 1);
    assert_eq!(smalls[0].logical, 30);
    assert_eq!(pruned.logical, r.node.logical);
    assert_eq!(pruned.children.len(), 2); // big.txt + synthetic smalls
}

#[test]
fn cleanup_guard_and_apply_trash() {
    assert!(check_protected(Path::new("/System/Library")).is_err());
    assert!(check_protected(Path::new("/usr/local/bin")).is_err());

    let td = tmpdir("clean");
    let root = td.path().join("caches");
    write(&root.join("junk.bin"), &[0u8; 2048]);
    let logical = 2048;

    let items = vec![StagedItem {
        path: root.clone(),
        logical,
        category: "user_caches".into(),
        reason: "test".into(),
    }];
    // Dry run touches nothing.
    let out = apply(&items, Disposal::Trash, true);
    assert!(out[0].ok);
    assert!(root.exists());

    // Real run removes the source (moves it into the user trash).
    let out = apply(&items, Disposal::Trash, false);
    assert!(out[0].ok, "{:?}", out[0].message);
    assert!(!root.exists());
}

#[test]
fn queue_rejects_protected_and_overlapping() {
    let mut q = CleanupQueue::default();
    assert!(q
        .stage(StagedItem {
            path: PathBuf::from("/usr"),
            logical: 0,
            category: "manual".into(),
            reason: "no".into(),
        })
        .is_err());

    let td = tmpdir("queue");
    let a = td.path().join("a");
    fs::create_dir_all(&a).unwrap();
    let ab = a.join("b");
    fs::create_dir_all(&ab).unwrap();
    q.stage(StagedItem {
        path: a.clone(),
        logical: 1,
        category: "manual".into(),
        reason: "t".into(),
    })
    .unwrap();
    assert!(
        q.stage(StagedItem {
            path: ab,
            logical: 1,
            category: "manual".into(),
            reason: "t".into()
        })
        .is_err(),
        "descendant of staged dir must be refused"
    );
    assert_eq!(q.items.len(), 1);
}
#[test]
fn dev_junk_found_without_double_counting() {
    let td = tmpdir("junk");
    let root = td.path();
    write(&root.join("proj/target/x/index.js"), &[0u8; 500]);
    write(&root.join("proj/target/x/deep/y.js"), &[0u8; 500]);
    write(
        &root.join("proj/node_modules/left-pad/index.js"),
        &[0u8; 500],
    );
    write(&root.join("proj/src/main.rs"), &[0u8; 100]);
    let r = scan_dir(root);
    let hits = find_dev_junk(&r);
    assert_eq!(hits.len(), 1, "node_modules must not be detected");
    assert_eq!(hits[0].logical, 1000);
    assert!(hits[0].path.ends_with("target"));
}

use std::path::PathBuf;
