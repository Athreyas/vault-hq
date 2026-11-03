//! Stream A end to end through `Hq`, on a temp vault built here.

use std::path::Path;
use std::sync::Arc;

use hq_core::hq::Hq;
use hq_core::types::{CaptureKind, NoteFilter, NoteInput, VaultLayout};
use hq_core::vault::layout;
use hq_core::Error;

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap()
}

struct Fixture {
    _dirs: Vec<tempfile::TempDir>,
    hq: Arc<Hq>,
    root: std::path::PathBuf,
    vault: String,
}

fn fixture() -> Fixture {
    let support = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let vaults = tempfile::tempdir().unwrap();
    let root = vaults.path().join("home");
    let hq = Hq::open(support.path(), cache.path()).unwrap();
    let info = hq
        .vault_create(root.to_str().unwrap(), "home", VaultLayout::Default)
        .unwrap();
    assert!(info.primary);
    write(
        &root,
        "01-Projects/garden.md",
        "---\ntitle: Garden\ntype: project\nstatus: active\nupdated: 2026-01-01\n---\n\n# Garden\n\nZones in [[zone-map]], tools in [the shed](../05-References/Shed%20List.md).\n\n## Tasks\n- [ ] Lay the line ⏫ 📅 2020-01-01 🆔 lay001\n",
    );
    write(
        &root,
        "05-References/zone-map.md",
        "---\ntitle: Zone map\n---\nBack to [[garden|the garden]] and [[01-Projects/garden#Tasks]].\n",
    );
    write(&root, "05-References/Shed List.md", "rake\n");
    write(
        &root,
        "04-Personal/garden-notes.md",
        "unrelated, links [[Zone-Map]]\n",
    );
    write(
        &root,
        "_templates/meeting.md",
        "# {{title}}\n\nHeld {{date}}.\n",
    );
    hq.invalidate(None);
    Fixture {
        vault: info.id,
        root,
        hq: Arc::new(hq),
        _dirs: vec![support, cache, vaults],
    }
}

#[test]
fn links_and_backlinks_resolve_inside_the_vault() {
    let f = fixture();
    let v = f.hq.note_get("garden").unwrap();
    assert_eq!(v.note.title, "Garden");
    let resolved: Vec<_> = v.links.iter().map(|l| l.resolved.clone()).collect();
    assert_eq!(
        resolved,
        vec![
            Some(format!("{}:05-References/zone-map.md", f.vault)),
            Some(format!("{}:05-References/Shed List.md", f.vault)),
        ]
    );
    assert_eq!(v.backlinks.len(), 1);
    assert_eq!(v.backlinks[0].rel, "05-References/zone-map.md");
    // case-insensitive stem, and a target outside the vault never resolves
    assert!(f.hq.note_get("ZONE-MAP").is_ok());
    assert!(matches!(
        f.hq.note_get("../../etc/hosts"),
        Err(Error::NotFound(_))
    ));
    assert!(f
        .hq
        .note_source(&format!("{}:../outside.md", f.vault))
        .is_err());
    let projects = f.hq.notes_list(&NoteFilter {
        note_type: Some("project".into()),
        ..Default::default()
    });
    assert_eq!(projects.len(), 1);
}

#[test]
fn guarded_save_returns_the_current_text_on_conflict() {
    let f = fixture();
    let id = format!("{}:05-References/zone-map.md", f.vault);
    let src = f.hq.note_source(&id).unwrap();
    // someone else writes in between
    std::fs::write(f.root.join("05-References/zone-map.md"), "agent edit\n").unwrap();
    let r = f.hq.note_save(&id, "my edit\n", &src.hash).unwrap();
    assert!(r.conflict);
    assert_eq!(r.text, "agent edit\n");
    assert_eq!(read(&f.root, "05-References/zone-map.md"), "agent edit\n");
    let ok = f.hq.note_save(&id, "merged\n", &r.hash).unwrap();
    assert!(!ok.conflict);
    assert_eq!(read(&f.root, "05-References/zone-map.md"), "merged\n");
}

#[test]
fn create_never_overwrites() {
    let f = fixture();
    let input = NoteInput {
        title: "Zone map".into(),
        folder: Some("05-References".into()),
        ..Default::default()
    };
    let before = read(&f.root, "05-References/zone-map.md");
    let v = f.hq.note_create(&input).unwrap();
    assert_eq!(v.note.rel, "05-References/zone-map-2.md");
    assert_eq!(read(&f.root, "05-References/zone-map.md"), before);
    let t =
        f.hq.note_create(&NoteInput {
            title: "Weekly sync".into(),
            template: Some("meeting".into()),
            ..Default::default()
        })
        .unwrap();
    let text = read(&f.root, &t.note.rel);
    assert!(text.starts_with("---\ntitle: Weekly sync\ntype: note\ncreated: "));
    assert!(text.contains("# Weekly sync\n\nHeld "));
    assert_eq!(f.hq.templates_list().len(), 1);
}

#[test]
fn move_updates_links_pointing_at_it() {
    let f = fixture();
    let id = format!("{}:01-Projects/garden.md", f.vault);
    let dry = f.hq.note_move(&id, "02-Areas/yard.md", true).unwrap();
    assert_eq!(
        dry.relinked,
        vec![format!("{}:05-References/zone-map.md", f.vault)]
    );
    assert!(f.root.join("01-Projects/garden.md").exists());
    let r = f.hq.note_move(&id, "02-Areas/yard.md", false).unwrap();
    assert_eq!(r.id, format!("{}:02-Areas/yard.md", f.vault));
    let zone = read(&f.root, "05-References/zone-map.md");
    assert!(
        zone.contains("[[yard|the garden]] and [[02-Areas/yard#Tasks]]"),
        "{zone}"
    );
    let moved = read(&f.root, "02-Areas/yard.md");
    // its own relative link still reaches the shed list from the new folder
    assert!(
        moved.contains("[the shed](../05-References/Shed%20List.md)"),
        "{moved}"
    );
    assert!(
        !moved.contains("updated: 2026-01-01"),
        "updated: was bumped"
    );
    assert_eq!(f.hq.note_get("yard").unwrap().backlinks.len(), 1);
    // archive moves it on, links follow again
    let a = f.hq.note_archive(&r.id).unwrap();
    assert_eq!(a.id, format!("{}:08-Archive/02-Areas/yard.md", f.vault));
    assert!(
        read(&f.root, "05-References/zone-map.md").contains("[[08-Archive/02-Areas/yard#Tasks]]")
    );
}

#[test]
fn blocked_vault_is_refused_before_reading() {
    let support = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let vaults = tempfile::tempdir().unwrap();
    let hq = Hq::open(support.path(), cache.path()).unwrap();
    hq.settings_update(serde_json::json!({ "blockedVaultNames": ["private-vault"] }))
        .unwrap();
    let blocked = vaults.path().join("private-vault");
    write(&blocked, "note.md", "secret\n");
    let err = hq.vault_add(blocked.to_str().unwrap()).unwrap_err();
    assert_eq!(err.kind(), "Denied");
    assert!(
        !blocked.join(".vault-hq").exists(),
        "nothing written inside"
    );
    let err = hq
        .vault_create(
            blocked.join("sub").to_str().unwrap(),
            "sub",
            VaultLayout::Default,
        )
        .unwrap_err();
    assert_eq!(err.kind(), "Denied");
    assert!(!blocked.join("sub").exists());
    assert!(hq.vaults().is_empty());

    // a vault that was fine, blocked later: it disappears from the open set
    let ok = vaults.path().join("fine");
    std::fs::create_dir(&ok).unwrap();
    hq.vault_add(ok.to_str().unwrap()).unwrap();
    assert_eq!(hq.vaults().len(), 1);
    hq.settings_update(serde_json::json!({ "blockedVaultNames": ["private-vault", "fine"] }))
        .unwrap();
    assert!(hq.vaults().is_empty());
    assert_eq!(hq.status().problems.len(), 1);
}

#[test]
fn capture_appends_safely_from_many_writers() {
    let f = fixture();
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let hq = f.hq.clone();
            std::thread::spawn(move || {
                for j in 0..10 {
                    hq.capture(&format!("thought {i}-{j}"), None, None).unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let rel = layout::capture_rel(&hq_core::util::today());
    let text = read(&f.root, &rel);
    assert!(text.starts_with("---\ntitle: "));
    assert_eq!(text.matches("type: capture").count(), 1, "one header");
    let lines: Vec<_> = text.lines().filter(|l| l.contains(" thought ")).collect();
    assert_eq!(lines.len(), 80, "every capture landed once");
    assert!(matches!(
        f.hq.capture("buy seeds", Some(CaptureKind::Task), None),
        Err(Error::Unavailable(_))
    ));
    assert!(matches!(
        f.hq.capture("https://example.com", None, None),
        Err(Error::Unavailable(_))
    ));
    assert_eq!(f.hq.inbox().len(), 1);
}

#[test]
fn home_needs_and_decisions() {
    let f = fixture();
    let needs = f.hq.needs();
    let overdue = needs.iter().find(|n| n.title == "Lay the line").unwrap();
    assert_eq!(overdue.task_id.as_deref(), Some("lay001"));
    let r = f.hq.needs_decide(&overdue.id, "snooze", "").unwrap();
    assert_eq!(r.result, "Snoozed for 3 days");
    assert!(f.hq.needs().iter().all(|n| n.id != overdue.id));
    assert_eq!(f.hq.settings().snoozed.len(), 1);
    let home = f.hq.home(None).unwrap();
    assert!(home
        .cards
        .iter()
        .any(|c| c.kind == "project" && c.title == "Garden"));
    assert!(f.hq.home(Some("not a date")).is_err());
    assert_eq!(f.hq.days("2026-11-01", 7).unwrap().len(), 7);
    let status = f.hq.status();
    assert_eq!(status.vaults.len(), 1);
    assert!(status.tips.tasks_overdue >= 1);
}

#[test]
fn watcher_invalidates_and_reports() {
    let f = fixture();
    let (tx, rx) = std::sync::mpsc::channel();
    let _w =
        f.hq.watch(move |vault, rels| {
            let _ = tx.send((vault.to_string(), rels));
        })
        .unwrap();
    assert_eq!(f.hq.notes_list(&NoteFilter::default()).len(), 4);
    std::thread::sleep(std::time::Duration::from_millis(300));
    write(&f.root, "00-Inbox/new.md", "hello\n");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let (vault, rels) = rx.recv_timeout(left).expect("a change event");
        assert_eq!(vault, f.vault);
        if rels.iter().any(|r| r == "00-Inbox/new.md") {
            break;
        }
    }
    assert_eq!(
        f.hq.notes_list(&NoteFilter::default()).len(),
        5,
        "rescanned"
    );
}
