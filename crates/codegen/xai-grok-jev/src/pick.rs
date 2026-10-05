//! Optional fail-open file pick after grep / list_dir (speed-plan rank 1).
//!
//! Chooses one candidate path. Faults, `none`, unknown labels, and 0–1
//! candidates are [`FilePickVerdict::NoPick`]. Does not hide other hits.

use std::collections::BTreeMap;

use crate::client::{Client, Settings};
use crate::metrics::{DecideRecord, DecideSource};
use crate::types::{Question, choice_q, decode_choice};

pub const MAX_CANDIDATES: usize = 8;
pub const MIN_CANDIDATES: usize = 2;
pub const NONE_LABEL: &str = "none";
pub const PICK_ANSWER_NAME: &str = "pick";
pub const PICK_INSTRUCTIONS: &str =
    "Which path should be read first to answer the task? Pick none if no single file stands out.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilePickVerdict {
    NoPick,
    Pick(String),
}

impl FilePickVerdict {
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::Pick(path) => Some(path),
            Self::NoPick => None,
        }
    }
}

pub fn reminder_text(path: &str) -> String {
    format!(
        "Jev picked `{path}` as the most relevant hit. Read that first. Other hits are still listed — this is a hint, not a filter."
    )
}

fn unique_paths(paths: impl IntoIterator<Item = impl Into<String>>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for path in paths {
        let path = path.into();
        let trimmed = path.trim();
        if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
            continue;
        }
        if seen.insert(trimmed.to_owned()) {
            out.push(trimmed.to_owned());
        }
        if out.len() == MAX_CANDIDATES {
            break;
        }
    }
    out
}

/// Dedup and cap. Returns empty when there are not enough distinct paths.
pub fn candidates(paths: impl IntoIterator<Item = impl Into<String>>) -> Vec<String> {
    let out = unique_paths(paths);
    if out.len() < MIN_CANDIDATES {
        Vec::new()
    } else {
        out
    }
}

pub fn pick_question(paths: &[String]) -> Question {
    let mut criteria: Vec<(String, String)> = paths
        .iter()
        .map(|path| (path.clone(), format!("Read {path} first")))
        .collect();
    criteria.push((
        NONE_LABEL.to_owned(),
        "No single file stands out; keep looking".to_owned(),
    ));
    choice_q(PICK_INSTRUCTIONS, criteria)
}

/// Fail-open: only [`FilePickVerdict::Pick`] on a live label that is one of `paths`.
pub async fn pick_file(client: &Client, paths: &[String]) -> FilePickVerdict {
    pick_file_recorded(client, paths).await.0
}

pub async fn pick_file_recorded(
    client: &Client,
    paths: &[String],
) -> (FilePickVerdict, Option<DecideRecord>) {
    let paths = candidates(paths.iter().cloned());
    if paths.is_empty() {
        return (FilePickVerdict::NoPick, None);
    }
    let mut questions = BTreeMap::new();
    questions.insert(PICK_ANSWER_NAME.to_owned(), pick_question(&paths));
    let state = serde_json::json!({ "paths": paths });
    let (response, record) = client
        .decide_timed(&state, &questions, DecideSource::FilePick)
        .await;
    let verdict = match response {
        Err(_) => FilePickVerdict::NoPick,
        Ok(response) => match response
            .answers
            .get(PICK_ANSWER_NAME)
            .and_then(|raw| decode_choice(raw).ok())
        {
            Some(choice) if paths.iter().any(|p| p == &choice.choice) => {
                FilePickVerdict::Pick(choice.choice)
            }
            _ => FilePickVerdict::NoPick,
        },
    };
    (verdict, Some(record))
}

pub async fn maybe_pick_file(
    settings: Option<&Settings>,
    client: Option<&Client>,
    paths: &[String],
) -> FilePickVerdict {
    maybe_pick_file_recorded(settings, client, paths).await.0
}

pub async fn maybe_pick_file_recorded(
    settings: Option<&Settings>,
    client: Option<&Client>,
    paths: &[String],
) -> (FilePickVerdict, Option<DecideRecord>) {
    if !settings.is_some_and(|s| s.file_pick_active()) {
        return (FilePickVerdict::NoPick, None);
    }
    let Some(client) = client else {
        return (FilePickVerdict::NoPick, None);
    };
    pick_file_recorded(client, paths).await
}

#[cfg(test)]
#[path = "pick_tests.rs"]
mod tests;
