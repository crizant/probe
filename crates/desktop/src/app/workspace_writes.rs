use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkspaceWrite {
    Request,
    Environment,
    Documentation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkspaceWriteStep {
    Wait,
    Start(WorkspaceWrite),
    ContinuePendingClose,
}

/// Snapshot of the queued workspace writes. At most one write runs at a time.
#[derive(Clone, Copy, Debug, Default)]
struct WorkspaceWriteQueues {
    busy: bool,
    requests: bool,
    environment: bool,
    documentation: bool,
}

impl WorkspaceWriteQueues {
    fn next_step(self) -> WorkspaceWriteStep {
        if self.busy {
            WorkspaceWriteStep::Wait
        } else if self.requests {
            WorkspaceWriteStep::Start(WorkspaceWrite::Request)
        } else if self.environment {
            WorkspaceWriteStep::Start(WorkspaceWrite::Environment)
        } else if self.documentation {
            WorkspaceWriteStep::Start(WorkspaceWrite::Documentation)
        } else {
            WorkspaceWriteStep::ContinuePendingClose
        }
    }
}

impl ProbeApp {
    /// Starts the next queued workspace write, or continues a pending close once
    /// every queue is drained. Call after enqueueing work or finishing a write.
    pub(super) fn pump_workspace_writes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        loop {
            let flow = match self.workspace_write_queues().next_step() {
                WorkspaceWriteStep::Wait => return,
                WorkspaceWriteStep::Start(WorkspaceWrite::Request) => {
                    self.start_request_save(window, cx)
                }
                WorkspaceWriteStep::Start(WorkspaceWrite::Environment) => {
                    self.start_environment_save(window, cx)
                }
                WorkspaceWriteStep::Start(WorkspaceWrite::Documentation) => {
                    self.start_documentation_save(window, cx)
                }
                WorkspaceWriteStep::ContinuePendingClose => {
                    self.continue_pending_close(window, cx);
                    return;
                }
            };
            if flow.is_break() {
                return;
            }
        }
    }

    fn workspace_write_queues(&self) -> WorkspaceWriteQueues {
        WorkspaceWriteQueues {
            busy: self.loading || self.has_active_workspace_write(),
            requests: self.persistence.has_outstanding_saves(),
            environment: !self.pending_environment_saves.is_empty(),
            documentation: !self.pending_documentation_saves.is_empty(),
        }
    }

    fn continue_pending_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pending) = self.pending_close.take() {
            let dirty = self.pending_close_dirty_keys(&pending);
            if dirty.is_empty() && self.pending_overview_targets(&pending).is_empty() {
                self.finish_pending_close(pending, window, cx);
            } else {
                self.prompt_unsaved(dirty, pending, window, cx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{WorkspaceWrite, WorkspaceWriteQueues, WorkspaceWriteStep};

    #[test]
    fn an_active_write_or_load_holds_every_queue_and_the_pending_close() {
        let queues = WorkspaceWriteQueues {
            busy: true,
            requests: true,
            environment: true,
            documentation: true,
        };
        assert_eq!(queues.next_step(), WorkspaceWriteStep::Wait);
        assert_eq!(
            WorkspaceWriteQueues {
                busy: true,
                ..WorkspaceWriteQueues::default()
            }
            .next_step(),
            WorkspaceWriteStep::Wait
        );
    }

    #[test]
    fn queues_drain_requests_then_environment_then_documentation() {
        let mut queues = WorkspaceWriteQueues {
            busy: false,
            requests: true,
            environment: true,
            documentation: true,
        };
        assert_eq!(
            queues.next_step(),
            WorkspaceWriteStep::Start(WorkspaceWrite::Request)
        );
        queues.requests = false;
        assert_eq!(
            queues.next_step(),
            WorkspaceWriteStep::Start(WorkspaceWrite::Environment)
        );
        queues.environment = false;
        assert_eq!(
            queues.next_step(),
            WorkspaceWriteStep::Start(WorkspaceWrite::Documentation)
        );
    }

    #[test]
    fn pending_close_continues_only_when_every_queue_is_idle() {
        assert_eq!(
            WorkspaceWriteQueues::default().next_step(),
            WorkspaceWriteStep::ContinuePendingClose
        );
    }
}
