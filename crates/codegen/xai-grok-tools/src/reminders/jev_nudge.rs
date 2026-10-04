//! Optional self-validation nudge after a successful mutate.
//!
//! No HTTP. The agent may call `ask_jev`; this reminder never does. Absent
//! [`JevNudgeConfig`] (no key, or `jev.nudge` off) this is a no-op.

use crate::types::output::{ApplyPatchOutput, SearchReplaceOutput, ToolOutput};
use crate::types::resources::SharedResources;
use crate::types::tool::Reminder;

/// Inserted only when `jev_enabled() && nudge && nudge_every > 0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JevNudgeConfig {
    pub every: i32,
}

/// Successful-mutate counter. Lives in Resources so rate-limit spans the turn.
#[derive(Debug, Default)]
pub struct JevNudgeState {
    pub successful_mutates: u32,
}

/// Cross-cutting reminder. Registered always; emits only when config is present.
pub struct JevNudgeReminder;

/// Hint appended to a successful edit/write/allowed shell. No Decide call.
pub const JEV_NUDGE_TEXT: &str = "If this change was a fix, consider ask_jev before declaring done \
(boolean: is the failure gone? score/boolean: too risky?). You decide whether to call it.";

fn is_successful_mutate(output: &ToolOutput) -> bool {
    match output {
        ToolOutput::SearchReplace(SearchReplaceOutput::EditsApplied(_)) => true,
        ToolOutput::ApplyPatch(ApplyPatchOutput::Success { .. }) => true,
        ToolOutput::Bash(b) if b.exit_code == 0 => true,
        _ => false,
    }
}

#[async_trait::async_trait]
impl Reminder for JevNudgeReminder {
    async fn collect_reminders(
        &self,
        resources: SharedResources,
        tool_output: &ToolOutput,
    ) -> Vec<String> {
        if !is_successful_mutate(tool_output) {
            return Vec::new();
        }
        let mut res = resources.lock().await;
        let Some(config) = res.get::<JevNudgeConfig>().copied() else {
            return Vec::new();
        };
        if config.every <= 0 {
            return Vec::new();
        }
        let every = config.every as u32;
        let state = res.get_or_default::<JevNudgeState>();
        state.successful_mutates = state.successful_mutates.saturating_add(1);
        if every == 0 || state.successful_mutates % every != 0 {
            return Vec::new();
        }
        vec![JEV_NUDGE_TEXT.to_owned()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::output::{
        ApplyPatchOutput, BashOutput, SearchReplaceEditContextInformation,
        SearchReplaceEditsApplied, SearchReplaceOutput,
    };
    use crate::types::resources::Resources;
    use std::path::PathBuf;

    fn edit_ok() -> ToolOutput {
        ToolOutput::SearchReplace(SearchReplaceOutput::EditsApplied(
            SearchReplaceEditsApplied {
                old_string: "a".into(),
                new_string: "b".into(),
                tool_output_for_prompt: "ok".into(),
                tool_output_for_prompt_concise: None,
                absolute_path: PathBuf::from("/tmp/x.rs"),
                edits: SearchReplaceEditContextInformation::default(),
                patch: None,
                unicode_normalized: false,
            },
        ))
    }

    fn edit_fail() -> ToolOutput {
        ToolOutput::SearchReplace(SearchReplaceOutput::FileNotFound("missing".into()))
    }

    fn bash(exit: i32) -> ToolOutput {
        ToolOutput::Bash(BashOutput {
            output: vec![],
            output_for_prompt: String::new(),
            exit_code: exit,
            command: "true".into(),
            truncated: false,
            signal: None,
            timed_out: false,
            description: None,
            current_dir: "/tmp".into(),
            output_file: String::new(),
            total_bytes: 0,
            output_delta: None,
            was_bare_echo: false,
        })
    }

    fn read_ok() -> ToolOutput {
        ToolOutput::Text("file contents".into())
    }

    async fn collect(res: Resources, output: &ToolOutput) -> Vec<String> {
        JevNudgeReminder
            .collect_reminders(res.into_shared(), output)
            .await
    }

    #[tokio::test]
    async fn no_config_is_silent() {
        let res = Resources::new();
        assert!(collect(res, &edit_ok()).await.is_empty());
    }

    #[tokio::test]
    async fn key_set_nudge_off_is_silent() {
        // Config is only inserted when nudge is on. Absent config == off.
        let res = Resources::new();
        assert!(collect(res, &edit_ok()).await.is_empty());
        assert!(collect(Resources::new(), &bash(0)).await.is_empty());
    }

    #[tokio::test]
    async fn every_two_fires_on_second_successful_mutate() {
        let mut res = Resources::new();
        res.insert(JevNudgeConfig { every: 2 });
        let shared = res.into_shared();
        let reminder = JevNudgeReminder;
        assert!(
            reminder
                .collect_reminders(shared.clone(), &edit_ok())
                .await
                .is_empty(),
            "first successful mutate must not nudge when every=2"
        );
        let second = reminder.collect_reminders(shared.clone(), &bash(0)).await;
        assert_eq!(second, vec![JEV_NUDGE_TEXT.to_owned()]);
        assert!(
            reminder
                .collect_reminders(shared, &edit_ok())
                .await
                .is_empty(),
            "third mutate is not a multiple of 2"
        );
    }

    #[tokio::test]
    async fn failed_edit_and_failed_shell_do_not_count() {
        let mut res = Resources::new();
        res.insert(JevNudgeConfig { every: 1 });
        let shared = res.into_shared();
        let reminder = JevNudgeReminder;
        assert!(
            reminder
                .collect_reminders(shared.clone(), &edit_fail())
                .await
                .is_empty()
        );
        assert!(
            reminder
                .collect_reminders(shared.clone(), &bash(1))
                .await
                .is_empty()
        );
        assert!(
            reminder
                .collect_reminders(shared, &read_ok())
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn every_nonpositive_disables() {
        let mut res = Resources::new();
        res.insert(JevNudgeConfig { every: -1 });
        assert!(collect(res, &edit_ok()).await.is_empty());
    }

    fn apply_patch_ok() -> ToolOutput {
        ToolOutput::ApplyPatch(ApplyPatchOutput::Success {
            files: vec![],
            tool_output_for_prompt: "ok".into(),
        })
    }

    #[tokio::test]
    async fn apply_patch_success_counts_as_mutate() {
        let mut res = Resources::new();
        res.insert(JevNudgeConfig { every: 1 });
        assert_eq!(
            collect(res, &apply_patch_ok()).await,
            vec![JEV_NUDGE_TEXT.to_owned()]
        );
    }
}
