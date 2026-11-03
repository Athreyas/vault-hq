//! Reading an action item with the local model, through Ollama's structured output: the
//! reply must follow `schema()`, so a small model can't ramble. The answer is a proposal;
//! `super::reconcile` checks it against HQ's own date parser before the user sees it.

use chrono::{Duration, NaiveDateTime};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::Result;
use crate::ollama::{ChatMessage, JsonChat};

/// What the model said, fields not yet trusted.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Extracted {
    pub is_action: bool,
    pub description: String,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub project: Option<String>,
    pub tags: Vec<String>,
    pub priority: Option<String>,
    pub reminder: bool,
}

pub fn schema() -> Value {
    let nullable = |t: &str| json!({ "type": [t, "null"] });
    json!({
        "type": "object",
        "properties": {
            "is_action": { "type": "boolean" },
            "description": { "type": "string" },
            "due_date": nullable("string"),
            "due_time": nullable("string"),
            "project": nullable("string"),
            "tags": { "type": "array", "items": { "type": "string" } },
            "priority": { "type": ["string", "null"],
                "enum": ["highest", "high", "medium", "low", "lowest", null] },
            "reminder": { "type": "boolean" }
        },
        "required": ["is_action", "description", "due_date", "due_time", "project",
            "tags", "priority", "reminder"]
    })
}

/// The prompt: today's date and the next week spelled out, since small models miscount
/// weekdays more often than they misread the request.
pub fn messages(text: &str, now: NaiveDateTime) -> Vec<ChatMessage> {
    let today = now.date();
    let week: Vec<String> = (0..8)
        .map(|i| {
            let d = today + Duration::days(i);
            format!("{} {}", d.format("%A"), d.format("%Y-%m-%d"))
        })
        .collect();
    let system = format!(
        "You turn one message into a task for a to-do list, or say it is not one.\n\
         Now: {} {} (local time). The next days: {}.\n\
         Rules:\n\
         - is_action: true only if the message asks to add, create or remember something to do.\n\
           Questions and statements are not actions.\n\
         - description: the thing to do, short and imperative, without the date, time,\n\
           \"remind me\" or \"add a task\" words. Keep names exactly as written.\n\
         - due_date: YYYY-MM-DD, or null if no day is given. Resolve words like tomorrow,\n\
           Friday, next week against the dates above.\n\
         - due_time: HH:MM in 24-hour time, or null if no time is given (5PM is 17:00).\n\
         - project: the project or app it belongs to if the message names one, else null.\n\
         - tags: at most three short lowercase words, only if obvious; usually [].\n\
         - priority: only if the message says urgent, important, high or low priority; else null.\n\
         - reminder: true if the user asks to be reminded.",
        today.format("%A %Y-%m-%d"),
        now.format("%H:%M"),
        week.join(", "),
    );
    vec![
        ChatMessage::new("system", system),
        ChatMessage::new("user", text.to_string()),
    ]
}

/// Ask the model. Anything that doesn't fit the schema is an error; the caller falls back
/// to the rules.
pub fn extract(chat: &dyn JsonChat, text: &str, now: NaiveDateTime) -> Result<Extracted> {
    let v = chat.chat_json(&messages(text, now), &schema())?;
    Ok(serde_json::from_value(v)?)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::error::Error;
    use std::cell::RefCell;

    /// Answers with a fixed JSON value and records the prompt it got.
    pub(crate) struct FakeModel {
        pub reply: Result<Value>,
        pub seen: RefCell<Vec<Vec<ChatMessage>>>,
    }

    impl FakeModel {
        pub fn new(reply: Value) -> Self {
            FakeModel {
                reply: Ok(reply),
                seen: RefCell::new(vec![]),
            }
        }
        pub fn down() -> Self {
            FakeModel {
                reply: Err(Error::Unavailable("Ollama is not reachable".into())),
                seen: RefCell::new(vec![]),
            }
        }
    }

    impl JsonChat for FakeModel {
        fn model(&self) -> &str {
            "fake"
        }
        fn chat_json(&self, m: &[ChatMessage], schema: &Value) -> Result<Value> {
            assert_eq!(schema["required"].as_array().map(Vec::len), Some(8));
            self.seen.borrow_mut().push(m.to_vec());
            match &self.reply {
                Ok(v) => Ok(v.clone()),
                Err(e) => Err(Error::Unavailable(e.to_string())),
            }
        }
    }

    #[test]
    fn prompt_spells_out_the_week_and_parses_the_reply() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 11, 3)
            .unwrap()
            .and_hms_opt(10, 30, 0)
            .unwrap();
        let m = FakeModel::new(json!({
            "is_action": true, "description": "Call the bank", "due_date": "2026-11-06",
            "due_time": "09:00", "project": null, "tags": [], "priority": null, "reminder": true
        }));
        let e = extract(&m, "remind me to call the bank Friday 9am", now).unwrap();
        assert!(e.is_action && e.reminder);
        assert_eq!(e.due_time.as_deref(), Some("09:00"));
        let sys = &m.seen.borrow()[0][0].content;
        assert!(sys.contains("Tuesday 2026-11-03 10:30"), "{sys}");
        assert!(sys.contains("Friday 2026-11-06"));
        // a reply that isn't the shape asked for is an error, not a guess
        let bad = FakeModel::new(json!({"is_action": "maybe"}));
        assert!(extract(&bad, "x", now).is_err());
    }
}
