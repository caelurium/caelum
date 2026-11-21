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

## Remaining candidates

| Owner | State currently split across fields | Invariant to encode |
| --- | --- | --- |
| [Conversation activity and entries](../crates/caelum-core/src/application/conversation.rs) | Working/retrying flags, entry kind, completion, provider status, background | Model provider activity separately from timing. Give each entry kind the lifecycle and payload it supports; accept unknown provider statuses at the decoding boundary. |
| [Review and service requests](../crates/caelum-core/src/application/services.rs) | Review source and details are separate; request slots share latest/loading/error maps | Tie cached/live details and file pages to a verified revision. A loading slot owns its request; refresh can retain visible data while a new request is pending. |
| [Desktop worktree form](../apps/desktop/src/worktree.rs) and [panels](../apps/desktop/src/panels.rs) | Form initialized/submitted flags, screen tags and optional payloads, file/search/review bookkeeping | Bind overlay contents and resource owners to their screens. In-flight creation must retain the submitted task rather than infer it from mutable fields. |
| [Desktop completion](../apps/desktop/src/completion.rs), [sidebar](../apps/desktop/src/live/sidebar.rs), and [remote UI](../apps/desktop/src/remote/mod.rs) | Pending tuples, scheduled deadlines, optional owner-specific UI data | Keep request and timer identities with the states they can advance. Sort out independent focus, navigation, and rendering state before merging fields. |
| [Desktop preview](../apps/desktop/src/state.rs) | Optional stream and separate turn flags | A streaming preview response owns its turn and clock rather than acting on whichever turn is last. |

Updates, OpenCode connection slots, conversation controls, relocation, and remote
checkout and turn lifecycles already use phase enums. Review their event
admission separately from representation changes. Persisted snapshots and
provider protocol structs are compatibility boundaries, not authoritative
runtime state.
