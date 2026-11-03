//! The default folder layout of a vault (SPEC 3). HQ reads any folder; these are the
//! places it writes to and the names its views look for. Onboarding (stream E) writes
//! new-vault scaffolding from these.

pub const INBOX: &str = "00-Inbox";
pub const PROJECTS: &str = "01-Projects";
pub const AREAS: &str = "02-Areas";
pub const LEARNING: &str = "03-Learning";
pub const PERSONAL: &str = "04-Personal";
pub const REFERENCES: &str = "05-References";
pub const MEMORY: &str = "06-Memory";
pub const AGENTS: &str = "07-Agents";
pub const ARCHIVE: &str = "08-Archive";
pub const PLANNING: &str = "09-Planning";
pub const BOOKMARKS: &str = "10-Bookmarks";
pub const TEMPLATES: &str = "_templates";

/// One `YYYY-MM-DD.md` per day.
pub const DAILY: &str = "09-Planning/Daily";
pub const WEEKLY: &str = "09-Planning/Weekly";
pub const DECISION_LOG: &str = "09-Planning/decision-log.md";
/// Loose tasks that belong to no project.
pub const TASKS_NOTE: &str = "09-Planning/Tasks.md";
/// Where archived inbox items go.
pub const ARCHIVE_INBOX: &str = "08-Archive/inbox";
pub const AGENTS_MD: &str = "AGENTS.md";

/// Every default folder, parents before children.
pub const FOLDERS: &[&str] = &[
    INBOX, PROJECTS, AREAS, LEARNING, PERSONAL, REFERENCES, MEMORY, AGENTS, ARCHIVE, PLANNING,
    DAILY, WEEKLY, BOOKMARKS, TEMPLATES,
];

/// The daily note for a `YYYY-MM-DD` date.
pub fn daily_rel(date: &str) -> String {
    format!("{DAILY}/{date}.md")
}

/// The quick-capture note for a date (SPEC 5).
pub fn capture_rel(date: &str) -> String {
    format!("{INBOX}/{date}-capture.md")
}

/// Where a new note of `kind` goes when the caller names no folder.
pub fn folder_for_type(kind: &str) -> &'static str {
    match kind {
        "project" => PROJECTS,
        "area" => AREAS,
        "reference" | "findings" => REFERENCES,
        "memory" => MEMORY,
        "agent" => AGENTS,
        "daily" => DAILY,
        "bookmark" => BOOKMARKS,
        _ => INBOX,
    }
}

/// True for `rel` inside `folder` (a folder name without the trailing slash).
pub fn is_in(rel: &str, folder: &str) -> bool {
    rel.strip_prefix(folder)
        .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(daily_rel("2026-11-02"), "09-Planning/Daily/2026-11-02.md");
        assert_eq!(capture_rel("2026-11-02"), "00-Inbox/2026-11-02-capture.md");
        assert_eq!(folder_for_type("project"), "01-Projects");
        assert_eq!(folder_for_type("whatever"), "00-Inbox");
        assert!(is_in("00-Inbox/a.md", INBOX));
        assert!(!is_in("00-Inboxes/a.md", INBOX));
        assert!(!is_in("00-Inbox", INBOX));
    }
}
