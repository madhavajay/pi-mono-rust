use crate::coding_agent::ModelRegistry;
use crate::core::compaction::CompactionPreparation;
use crate::core::session_manager::{CompactionEntry, SessionEntry, SessionManager};
use serde::Serialize;

pub struct HookContext<'a> {
    pub session_manager: &'a SessionManager,
    pub model_registry: &'a ModelRegistry,
}

pub struct HookAPI;

impl Default for HookAPI {
    fn default() -> Self {
        Self::new()
    }
}

impl HookAPI {
    pub fn new() -> Self {
        Self
    }

    pub fn on_session_before_compact<F>(&self, _handler: F)
    where
        F: for<'a> Fn(&SessionBeforeCompactEvent, &HookContext<'a>) -> SessionBeforeCompactResult,
    {
    }

    pub fn on_session_compact<F>(&self, _handler: F)
    where
        F: for<'a> Fn(&SessionCompactEvent, &HookContext<'a>),
    {
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionBeforeCompactEvent {
    pub preparation: CompactionPreparation,
    pub branch_entries: Vec<SessionEntry>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionCompactEvent {
    pub compaction_entry: CompactionEntry,
    pub from_hook: bool,
}

type BeforeCompactHandler = Box<dyn Fn(&SessionBeforeCompactEvent) -> SessionBeforeCompactResult>;
type CompactHandler = Box<dyn Fn(&SessionCompactEvent)>;

pub struct CompactionHook {
    pub on_before_compact: Option<BeforeCompactHandler>,
    pub on_compact: Option<CompactHandler>,
}

impl CompactionHook {
    pub fn new(
        on_before_compact: Option<BeforeCompactHandler>,
        on_compact: Option<CompactHandler>,
    ) -> Self {
        Self {
            on_before_compact,
            on_compact,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CompactionResult {
    pub summary: String,
    pub first_kept_entry_id: String,
    pub tokens_before: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionBeforeCompactResult {
    pub cancel: Option<bool>,
    pub compaction: Option<CompactionResult>,
}
