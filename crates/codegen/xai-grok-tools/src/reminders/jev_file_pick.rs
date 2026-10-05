//! Optional file-pick hint after grep / list_dir.
//!
//! No-op unless [`JevFilePickConfig`] is in Resources (key + `file_pick`).
//! Hits stay in the tool result; this only appends a reminder.

use crate::types::output::{ListDirOutput, ToolOutput};
use crate::types::resources::SharedResources;
use crate::types::tool::Reminder;
use xai_grok_jev::{
    FilePickVerdict, JevMetrics, pick_file_recorded, reminder_text as pick_reminder_text,
};

/// Inserted only when `jev_enabled() && file_pick`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JevFilePickConfig;

pub struct JevFilePickReminder;

fn extract_paths(output: &ToolOutput) -> Vec<String> {
    match output {
        ToolOutput::GrepSearch(grep) => grep
            .file_matches
            .iter()
            .map(|m| m.path.clone())
            .collect(),
        ToolOutput::ListDir(ListDirOutput::Content(content)) => {
            parse_listing(&content.content, &content.absolute_root_path)
        }
        _ => Vec::new(),
    }
}

fn parse_listing(listing: &str, root: &std::path::Path) -> Vec<String> {
    listing
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.ends_with('/') || line.starts_with('-') {
                return None;
            }
            let name = line.split_whitespace().last()?;
            if name == "found" || name.contains("children") {
                return None;
            }
            Some(root.join(name).display().to_string())
        })
        .collect()
}

#[async_trait::async_trait]
impl Reminder for JevFilePickReminder {
    async fn collect_reminders(
        &self,
        resources: SharedResources,
        tool_output: &ToolOutput,
    ) -> Vec<String> {
        let (client, paths) = {
            let res = resources.lock().await;
            if res.get::<JevFilePickConfig>().is_none() {
                return Vec::new();
            }
            let client = res.get::<xai_grok_jev::Client>().cloned();
            (client, extract_paths(tool_output))
        };
        let Some(client) = client else {
            return Vec::new();
        };
        let (verdict, record) = pick_file_recorded(&client, &paths).await;
        if let Some(record) = record {
            let mut res = resources.lock().await;
            if let Some(metrics) = res.get_mut::<JevMetrics>() {
                metrics.record(record);
            }
        }
        match verdict {
            FilePickVerdict::Pick(path) => vec![pick_reminder_text(&path)],
            FilePickVerdict::NoPick => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::output::{GrepFileMatch, GrepSearchOutput, ListDirContent};
    use std::path::PathBuf;

    #[test]
    fn grep_extracts_file_matches() {
        let output = ToolOutput::GrepSearch(GrepSearchOutput {
            stdout: vec![],
            stderr: vec![],
            exit_code: 0,
            match_count: 2,
            file_matches: vec![
                GrepFileMatch {
                    path: "src/a.rs".into(),
                    matches: vec![],
                },
                GrepFileMatch {
                    path: "src/b.rs".into(),
                    matches: vec![],
                },
            ],
        });
        assert_eq!(extract_paths(&output), vec!["src/a.rs", "src/b.rs"]);
    }

    #[test]
    fn list_dir_extracts_file_lines() {
        let output = ToolOutput::ListDir(ListDirOutput::Content(ListDirContent {
            content: "- /tmp/proj/\n  src/\n  README.md\n  lib.rs".into(),
            absolute_root_path: PathBuf::from("/tmp/proj"),
        }));
        let paths = extract_paths(&output);
        assert!(paths.iter().any(|p| p.ends_with("README.md")), "{paths:?}");
        assert!(paths.iter().any(|p| p.ends_with("lib.rs")), "{paths:?}");
        assert!(!paths.iter().any(|p| p.ends_with("src")), "{paths:?}");
    }

    #[tokio::test]
    async fn flag_off_is_silent() {
        let resources = crate::types::resources::Resources::new().into_shared();
        let output = ToolOutput::GrepSearch(GrepSearchOutput {
            stdout: vec![],
            stderr: vec![],
            exit_code: 0,
            match_count: 2,
            file_matches: vec![
                GrepFileMatch {
                    path: "a.rs".into(),
                    matches: vec![],
                },
                GrepFileMatch {
                    path: "b.rs".into(),
                    matches: vec![],
                },
            ],
        });
        let texts = JevFilePickReminder
            .collect_reminders(resources, &output)
            .await;
        assert!(texts.is_empty());
    }
}
