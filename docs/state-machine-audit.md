# State machine audit

Encode a phase and its required data together. Keep independent lifecycles
separate: a session can fetch history while the provider is still executing.
Keep request identity checks at the point where asynchronous results enter the
state machine; types cannot establish that an external event is current.

## Converted

| Owner | Typed invariant |
| --- | --- |
| [Conversation turn timing](../crates/caelum-core/src/application/conversation/turn.rs) | Only an in-progress timing record has a start time, captured agent, and output IDs. Provider start is a separate phase that cannot reinitialize an already confirmed turn. |
| [OpenCode timing](../crates/caelum-core/src/application/conversation/opencode.rs) and [history reconstruction](../crates/caelum-core/src/application/conversation/timing.rs) | Step model and timestamp travel with the message ID; history keeps timing data only inside a reconstructed turn. |
| [Approvals](../crates/caelum-core/src/approvals.rs) and [processes](../crates/caelum-core/src/processes.rs) | Closed registries do not own pending approvals, subscribers, or children. Waiter cleanup also verifies the approval attempt, not just the provider request ID. |
| [CLI event handling](../apps/cli/src/main.rs) | A provider event has one outcome, and completion carries a typed terminal status rather than a string. |
| [Workspace persistence](../crates/caelum-core/src/application/workspace/persistence.rs) | Only a loaded store can save. A save owns its snapshot and held commands; only its matching completion can release them. |
| [Workspace history](../crates/caelum-core/src/application/workspace/history.rs) | An older-page read requires a loaded cursor. Resume and page reads own their request identity, and a refresh can keep previously loaded history without resetting its pagination. |
| [Archive notice](../crates/caelum-core/src/application/workspace/archive.rs) | The target, countdown and optional replacement only exist together as a visible notice. Undo failure restarts the countdown only for its own target. |
| [Workspace submission](../crates/caelum-core/src/application/workspace/submission.rs) | Creation and admission own the captured prompt, options, origin and stop intent. A stop in flight has its own request. Native commands have a separate owner and can overlap a provider turn. |
| [Session archive and deletion](../crates/caelum-core/src/application/workspace/archive.rs) | Changing visibility retains the previous stable phase for failure. Only an archived session can enter deletion, and a forgotten slot cannot receive a late mutation or history reply. |
| [Inbox](../crates/caelum-core/src/application/inbox.rs) | Enqueue and removal have distinct, request-owned phases. A failed operation can be retried without accepting another operation’s completion, and persisted snapshots keep their existing format. |
| [Session titles](../crates/caelum-core/src/application/workspace/titles.rs) | A manual title cannot accrue automatic refresh turns. Automatic updates own the request ID and captured session key. |
| [Questions](../crates/caelum-core/src/questions.rs) | Disabled brokers cannot hold requests. Only an awaiting question has a callback available to submit; its completion must match the exact answer attempt. |
| [Branch attribution](../crates/caelum-core/src/session_branches.rs) | Exclusive and ambiguous executions are distinct. Codex events require a turn ID; OpenCode terminal events older than the active start are ignored when timestamps are available. |
| [Desktop worktree form](../apps/desktop/src/worktree.rs) and [panels](../apps/desktop/src/panels.rs) | A submitted worktree owns its launch inputs; worktree and VM removal overlays own their payloads. A closed file find has no match cursor, and scroll restoration checks the active file or diff owner. |
| [Desktop completion](../apps/desktop/src/completion.rs), [sidebar](../apps/desktop/src/live/sidebar.rs), and [remote stats](../apps/desktop/src/remote/stats.rs) | Mention application owns the request, editor and captured selection. Sidebar recency timers exist only while active; auto-stop refresh is keyed to the machine and deadline. |
| [Desktop preview](../apps/desktop/src/state.rs) | A streaming response owns its clock and visible text; stopping consumes that phase. Only turns with tools have disclosure state. |
| [Conversation activity](../crates/caelum-core/src/application/conversation.rs) and [entries](../crates/caelum-core/src/application/conversation/entry.rs) | Idle cannot also be retrying. Only a user prompt can be awaiting its provider echo; only a tool can run in the background. Provider status labels remain display data, independent of the typed progress phase. |
| [Review snapshots](../crates/caelum-core/src/application/services/review.rs) and [service requests](../crates/caelum-core/src/application/services/requests.rs) | Cached or live details own file pages for that base and head. Pagination accepts only the expected next page. A running slot owns its request and revision pin; a failed slot has no active request, and read invalidation does not release a mutation lock. |
| [Side questions](../crates/caelum-core/src/application/workspace/side_question.rs) | Only an active streaming request owns a cancellation handle and receives deltas. Editing, a partial answer after failure, and a completed answer are distinct. Disconnect preserves the partial answer and retires the request. |
| [Project opening](../crates/caelum-core/src/application/workspace/projects.rs) | The loading indicator belongs to the most recent request. Older validated projects may still register, while selection requires the current request and navigation generation. |
| [Provider connection ownership](../crates/caelum-core/src/codex.rs), [OpenCode](../crates/caelum-core/src/opencode.rs), and [Antigravity](../crates/caelum-core/src/antigravity.rs) | Closed providers cannot retain registered checkout connections or admit a new one. Connection attempts that finish after shutdown retire their own resources. |
| [Antigravity turn progress](../crates/caelum-core/src/antigravity/progress.rs) and [saved visibility](../crates/caelum-core/src/antigravity/metadata.rs) | A prompt reply must match the active ACP request. Background work owns at least one tool. Settling owns a generation and has no active tools. A deleted session cannot also be archived. The metadata file keeps its existing fields. |
| [Image and attachment previews](../apps/desktop/src/panels/overlays.rs) | Each overlay owns the preview it displays, and the VM stats popup owns its inspection target. Image zoom and drag state belong to that image. Composer scroll restoration retains only the original editor and its offset. |
| [Runtime shutdown](../crates/caelum-core/src/application/runtime.rs) | The worker pool and its event inbox move out of the running state together. Shutdown releases blocked senders before joining workers. |

## Scope boundaries

Independent choices and measurements, such as sidebar visibility and token
counts, can coexist. Persisted snapshots and provider protocol values retain
their compatibility shapes and enter runtime state through conversion or
validation. The types encode which fields can coexist. Tests still cover
ordering, ownership, retry, and stale completions at asynchronous boundaries.
