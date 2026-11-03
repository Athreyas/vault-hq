//! Ask: a question answered by the local chat model from the vault's own notes.
//!
//! Events go out in a fixed order through the caller's callback: `Sources` (what was
//! read) first, then `Token`s as the model writes, then exactly one `Done` or `Error`.
//! The exchange is saved as a conversation note; a follow-up sends the last turns along.

use crate::error::{Error, Result};
use crate::ollama::{CancelToken, Chat, ChatMessage};
use crate::types::{split_note_id, AskEvent, SearchScope, Turn};
use crate::util;
use crate::vault::Vault;

use super::conversations::{self, FOLLOW_UP_TURNS};
use super::{hit, rank, Index, Semantic, TOP_K};

pub struct AskInput<'a> {
    pub question: &'a str,
    pub scope: &'a SearchScope,
    /// `"<vaultId>:<rel>"` of the conversation this follows up, if any.
    pub conversation: Option<&'a str>,
    pub today: &'a str,
}

/// Where conversations are read from and written to.
pub struct AskVaults<'a> {
    /// Every open vault (to find the conversation a follow-up belongs to).
    pub all: &'a [Vault],
    /// Where a new conversation is saved: the primary vault.
    pub save_to: Option<&'a Vault>,
}

fn system_prompt(today: &str) -> String {
    format!(
        "You answer questions about the user's own notes. Use ONLY the excerpts provided. \
         Notes from 01-Projects describe the current state; prefer them over session logs, \
         which describe the past. In checklists, '- [ ]' is still to do and '- [x]' is done. \
         Today is {today}. When excerpts disagree, the one with the most recent date wins; \
         words like 'was', 'replaced' or 'before' mark the older fact. \
         Cite the note for every fact as [[note-stem]], copied from the excerpt header. \
         If the excerpts do not answer the question, say plainly that the notes don't say. \
         Answer directly: a short paragraph or a few bullets, no preamble."
    )
}

/// The messages sent to the chat model: instructions, earlier turns, then the excerpts
/// and the question.
pub fn build_messages(
    index: &Index,
    picked: &[(usize, f64)],
    question: &str,
    history: &[Turn],
    today: &str,
) -> Vec<ChatMessage> {
    let context = picked
        .iter()
        .enumerate()
        .map(|(n, (i, _))| {
            let c = &index.chunks()[*i];
            let folder = c.rel.split('/').next().unwrap_or("");
            let date = c
                .newest_date()
                .map(|d| format!(", dated {d}"))
                .unwrap_or_default();
            format!(
                "Excerpt {}, from note [[{}]] ({folder}{date}), section \"{}\":\n{}",
                n + 1,
                c.stem,
                c.heading,
                c.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");
    let mut m = vec![ChatMessage::new("system", system_prompt(today))];
    let start = history.len().saturating_sub(FOLLOW_UP_TURNS);
    for t in &history[start..] {
        m.push(ChatMessage::new("user", t.question.clone()));
        if !t.answer.is_empty() {
            m.push(ChatMessage::new("assistant", t.answer.clone()));
        }
    }
    let context = if context.is_empty() {
        "(no matching notes)".to_string()
    } else {
        context
    };
    m.push(ChatMessage::new(
        "user",
        format!("Excerpts from the vault:\n\n{context}\n\nQuestion: {question}"),
    ));
    m
}

/// Small models cite "Excerpt 3" despite instructions; turn those into note links.
pub fn tidy_citations(answer: &str, stems: &[String]) -> String {
    let mut out = String::with_capacity(answer.len());
    let mut rest = answer.trim();
    while let Some(pos) = find_excerpt(rest) {
        let (before, from) = rest.split_at(pos);
        let word_len = if from[1..].starts_with("xcerpts ") {
            "Excerpts ".len()
        } else {
            "Excerpt ".len()
        };
        let digits: String = from[word_len..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let link = digits
            .parse::<usize>()
            .ok()
            .and_then(|n| n.checked_sub(1))
            .and_then(|n| stems.get(n));
        match link {
            Some(stem) if !digits.is_empty() => {
                // drop a "(" just before and a ")" just after, as in "(Excerpt 2)"
                let before_trim = before.strip_suffix('(');
                let after = &from[word_len + digits.len()..];
                let (b, a) = match (before_trim, after.strip_prefix(')')) {
                    (Some(b), Some(a)) => (b, a),
                    _ => (before, after),
                };
                out.push_str(b);
                out.push_str(&format!("[[{stem}]]"));
                rest = a;
            }
            _ => {
                out.push_str(&rest[..pos + word_len]);
                rest = &rest[pos + word_len..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The source stems the answer links to, in source order; every source if it links none.
pub fn cited_sources(answer: &str, stems: &[String]) -> Vec<String> {
    let mut all: Vec<String> = Vec::new();
    for s in stems {
        if !all.contains(s) {
            all.push(s.clone());
        }
    }
    let linked: Vec<String> = all
        .iter()
        .filter(|s| answer.contains(&format!("[[{s}]]")) || answer.contains(&format!("[[{s}|")))
        .cloned()
        .collect();
    if linked.is_empty() {
        all
    } else {
        linked
    }
}

fn find_excerpt(s: &str) -> Option<usize> {
    let a = s.find("Excerpt ");
    let b = s.find("excerpt ");
    let c = s.find("Excerpts ");
    let d = s.find("excerpts ");
    [a, b, c, d].into_iter().flatten().min()
}

/// The outcome the caller may want beyond the events (the MCP `ask` tool returns it).
#[derive(Debug, Clone, PartialEq)]
pub struct Answered {
    pub answer: String,
    pub model: String,
    pub stopped: bool,
    pub conversation: Option<String>,
}

/// Answer `input.question`, emitting events as it goes. Errors are emitted as an
/// `Error` event *and* returned, so a caller can ignore either.
pub fn ask(
    index: &Index,
    input: &AskInput<'_>,
    vaults: &AskVaults<'_>,
    semantic: Option<Semantic<'_>>,
    chat: Option<&dyn Chat>,
    cancel: &CancelToken,
    emit: &mut dyn FnMut(AskEvent),
) -> Result<Answered> {
    let r = run(index, input, vaults, semantic, chat, cancel, emit);
    if let Err(e) = &r {
        emit(AskEvent::Error {
            error: e.kind().into(),
            message: e.to_string(),
        });
    }
    r
}

fn run(
    index: &Index,
    input: &AskInput<'_>,
    vaults: &AskVaults<'_>,
    semantic: Option<Semantic<'_>>,
    chat: Option<&dyn Chat>,
    cancel: &CancelToken,
    emit: &mut dyn FnMut(AskEvent),
) -> Result<Answered> {
    let question = input.question.trim();
    if question.chars().count() < 2 {
        return Err(Error::Invalid("Ask a question first.".into()));
    }
    // a follow-up: load the turns so far; its vault is where the answer is saved
    let follow = match input.conversation {
        Some(id) => {
            let (v, rel) = split_note_id(id)
                .ok_or_else(|| Error::Invalid(format!("{id} is not a conversation id")))?;
            let vault = vaults
                .all
                .iter()
                .find(|x| x.id == v)
                .ok_or_else(|| Error::NotFound(format!("No open vault has id {v}")))?;
            let c = conversations::load(vault, rel)?;
            Some((vault, rel.to_string(), c.turns))
        }
        None => None,
    };
    let history: &[Turn] = follow.as_ref().map_or(&[], |f| &f.2);
    // "and when?" finds nothing alone: search with the previous question as well
    let retrieval = match history.last() {
        Some(prev) => format!("{} {question}", prev.question),
        None => question.to_string(),
    };
    let ranked = rank(
        index,
        &retrieval,
        input.scope,
        TOP_K,
        semantic,
        input.today,
        Some(cancel),
    )?;
    let sources: Vec<_> = ranked
        .picked
        .iter()
        .map(|(i, s)| hit(&index.chunks()[*i], *s))
        .collect();
    emit(AskEvent::Sources {
        sources: sources.clone(),
        semantic: ranked.semantic,
    });

    let chat = chat.ok_or_else(|| {
        Error::Unavailable(
            "No chat model is set up, so Ask can only search. Pick one in Settings → Local AI."
                .into(),
        )
    })?;
    if cancel.is_cancelled() {
        return Ok(Answered {
            answer: String::new(),
            model: chat.model().into(),
            stopped: true,
            conversation: input.conversation.map(String::from),
        });
    }
    let messages = build_messages(index, &ranked.picked, question, history, input.today);
    let streamed = chat.chat_stream(&messages, cancel, &mut |t| {
        emit(AskEvent::Token {
            text: t.to_string(),
        })
    })?;
    let stems: Vec<String> = sources.iter().map(|s| s.stem.clone()).collect();
    let answer = tidy_citations(&streamed.text, &stems);

    let cited = cited_sources(&answer, &stems);
    let turn = Turn {
        at: format!("{} {}", util::today(), util::now_hhmm()),
        question: question.to_string(),
        answer: answer.clone(),
        model: chat.model().to_string(),
        sources: cited,
    };
    // nothing said and stopped: nothing worth keeping
    let conversation = if streamed.stopped && answer.trim().is_empty() {
        input.conversation.map(String::from)
    } else {
        match &follow {
            Some((vault, rel, _)) => {
                conversations::append(vault, rel, &turn, streamed.stopped)?;
                input.conversation.map(String::from)
            }
            None => match vaults.save_to {
                Some(v) => Some(conversations::create(v, &turn, streamed.stopped)?),
                None => None,
            },
        }
    };
    emit(AskEvent::Done {
        answer: answer.clone(),
        model: chat.model().to_string(),
        stopped: streamed.stopped,
        conversation: conversation.clone(),
    });
    Ok(Answered {
        answer,
        model: chat.model().to_string(),
        stopped: streamed.stopped,
        conversation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ollama::{read_chat_stream_from, Streamed};
    use crate::search::tests::fixture;
    use std::cell::RefCell;

    /// Replays a canned NDJSON stream through the real parser and records what it was sent.
    struct Canned {
        stream: String,
        seen: RefCell<Vec<Vec<ChatMessage>>>,
    }

    impl Canned {
        fn new(tokens: &[&str]) -> Self {
            let mut s = String::new();
            for t in tokens {
                s.push_str(&serde_json::json!({"message": {"role": "assistant", "content": t}, "done": false}).to_string());
                s.push('\n');
            }
            s.push_str("{\"done\":true}\n");
            Canned {
                stream: s,
                seen: RefCell::new(vec![]),
            }
        }
    }

    impl Chat for Canned {
        fn model(&self) -> &str {
            "canned"
        }
        fn chat_stream(
            &self,
            messages: &[ChatMessage],
            cancel: &CancelToken,
            on_token: &mut dyn FnMut(&str),
        ) -> Result<Streamed> {
            self.seen.borrow_mut().push(messages.to_vec());
            read_chat_stream_from(self.stream.as_bytes(), cancel, on_token)
        }
    }

    fn kinds(ev: &[AskEvent]) -> Vec<&'static str> {
        ev.iter()
            .map(|e| match e {
                AskEvent::Sources { .. } => "sources",
                AskEvent::Token { .. } => "token",
                AskEvent::Done { .. } => "done",
                AskEvent::Error { .. } => "error",
            })
            .collect()
    }

    #[test]
    fn events_in_order_saved_and_followed_up() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        let all = vec![v];
        let vaults = AskVaults {
            all: &all,
            save_to: Some(&all[0]),
        };
        let idx = fixture();
        let chat = Canned::new(&["Along the fence ", "(Excerpt 1)", "."]);
        let mut ev = vec![];
        let scope = SearchScope::default();
        let a = ask(
            &idx,
            &AskInput {
                question: "Where does the drip line run?",
                scope: &scope,
                conversation: None,
                today: "2026-11-02",
            },
            &vaults,
            None,
            Some(&chat),
            &CancelToken::new(),
            &mut |e| ev.push(e),
        )
        .unwrap();
        assert_eq!(
            kinds(&ev),
            vec!["sources", "token", "token", "token", "done"]
        );
        assert_eq!(a.answer, "Along the fence [[garden]].");
        let AskEvent::Sources { sources, .. } = &ev[0] else {
            panic!()
        };
        assert_eq!(sources[0].stem, "garden");
        let id = a.conversation.clone().unwrap();
        let sent = &chat.seen.borrow()[0];
        assert_eq!(sent[0].role, "system");
        assert!(sent
            .last()
            .unwrap()
            .content
            .contains("Excerpt 1, from note [[garden]]"));

        // the follow-up sends the first turn and appends to the same note
        let chat2 = Canned::new(&["Before the frost [[garden]]."]);
        let mut ev2 = vec![];
        let b = ask(
            &idx,
            &AskInput {
                question: "and when?",
                scope: &scope,
                conversation: Some(&id),
                today: "2026-11-02",
            },
            &vaults,
            None,
            Some(&chat2),
            &CancelToken::new(),
            &mut |e| ev2.push(e),
        )
        .unwrap();
        assert_eq!(b.conversation.as_deref(), Some(id.as_str()));
        let sent = &chat2.seen.borrow()[0];
        let roles: Vec<&str> = sent.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, vec!["system", "user", "assistant", "user"]);
        assert_eq!(sent[1].content, "Where does the drip line run?");
        // retrieval used the earlier question too, so the garden note is still found
        let AskEvent::Sources { sources, .. } = &ev2[0] else {
            panic!()
        };
        assert_eq!(sources[0].stem, "garden");
        let c = conversations::get(&all, &id).unwrap();
        assert_eq!(c.turns.len(), 2);
        assert_eq!(c.turns[1].answer, "Before the frost [[garden]].");
    }

    #[test]
    fn no_chat_model_is_search_only_with_an_error() {
        let idx = fixture();
        let mut ev = vec![];
        let scope = SearchScope::default();
        let r = ask(
            &idx,
            &AskInput {
                question: "drip line?",
                scope: &scope,
                conversation: None,
                today: "2026-11-02",
            },
            &AskVaults {
                all: &[],
                save_to: None,
            },
            None,
            None,
            &CancelToken::new(),
            &mut |e| ev.push(e),
        );
        assert!(matches!(r, Err(Error::Unavailable(_))));
        assert_eq!(kinds(&ev), vec!["sources", "error"]);
        let AskEvent::Error { error, .. } = &ev[1] else {
            panic!()
        };
        assert_eq!(error, "Unavailable");
    }

    #[test]
    fn cancelled_before_answering() {
        let idx = fixture();
        let cancel = CancelToken::new();
        cancel.cancel();
        let chat = Canned::new(&["never"]);
        let mut ev = vec![];
        let scope = SearchScope::default();
        let a = ask(
            &idx,
            &AskInput {
                question: "drip line?",
                scope: &scope,
                conversation: None,
                today: "2026-11-02",
            },
            &AskVaults {
                all: &[],
                save_to: None,
            },
            None,
            Some(&chat),
            &cancel,
            &mut |e| ev.push(e),
        )
        .unwrap();
        assert!(a.stopped);
        assert!(chat.seen.borrow().is_empty());
        assert_eq!(kinds(&ev), vec!["sources"]);
    }

    #[test]
    fn bad_follow_up_ids_error() {
        let idx = fixture();
        let mut ev = vec![];
        let scope = SearchScope::default();
        let r = ask(
            &idx,
            &AskInput {
                question: "drip?",
                scope: &scope,
                conversation: Some("nope:06-Memory/conversations/x.md"),
                today: "2026-11-02",
            },
            &AskVaults {
                all: &[],
                save_to: None,
            },
            None,
            Some(&Canned::new(&["x"])),
            &CancelToken::new(),
            &mut |e| ev.push(e),
        );
        assert!(matches!(r, Err(Error::NotFound(_))));
        assert_eq!(kinds(&ev), vec!["error"]);
    }

    #[test]
    fn citations() {
        let stems = vec!["garden".to_string(), "fence".to_string()];
        assert_eq!(
            tidy_citations("Yes (Excerpt 2), see excerpt 1 and Excerpt 9.", &stems),
            "Yes [[fence]], see [[garden]] and Excerpt 9."
        );
        assert_eq!(tidy_citations("No excerpt here", &stems), "No excerpt here");
        assert_eq!(tidy_citations("Excerpts 1", &stems), "[[garden]]");
        assert_eq!(cited_sources("see [[fence]]", &stems), vec!["fence"]);
        assert_eq!(cited_sources("no links", &stems), stems);
    }
}
