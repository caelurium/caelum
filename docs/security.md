# Security boundaries

## Project filesystem access

- Rune selects files/directories with native dialogs and submits requests to the shared Rust application layer. Core owns project reads, Git operations and provider processes.
- Treat paths from dialogs, project URLs and provider data as untrusted. `Project::open` rejects empty paths, canonicalizes selected roots, and only accepts directories.
- Filesystem operations must resolve existing paths through `Project::resolve_scoped_path`. It canonicalizes the target and rejects paths, including symlinks, that escape the selected project.
- The scoped resolver intentionally covers existing paths only. Any future file-creation API must also defend against symlink races (for example, using directory-handle-relative operations) before it is exposed.

## Provider approvals

Provider adapters submit `PermissionRequest` values that retain structured provider data. The broker publishes the display fields, request owner, a fresh attempt ID and, when available, a narrow rule description. Raw provider payloads are not forwarded to the UI.

- **Allow similar in session** is offered only for recognized exact commands in the checkout root or single-file reads. Rules match structured requests, never display strings. File changes, additional privileges and unclassifiable requests remain manual.
- Rules live in memory, scoped to the checkout, provider and conversation. Each match receives a one-shot approval and a transcript entry; the session heading supports inspection and revocation. The broker verifies the pending attempt before accepting a decision, including when a provider reuses an ID.
- Timeouts, cancellation, app shutdown, unknown IDs, and dropped waiters resolve as denied.

The Codex adapter routes app-server command-execution and file-change approval requests through this broker. Caelum responds with a one-request `accept` or `decline`; unsupported request methods are rejected instead of being silently granted. New threads request Codex's `workspace-write` sandbox and `untrusted` approval policy. Review those provider-managed sandbox semantics separately from Caelum's own filesystem boundary.

The OpenCode adapter maps server `permission.asked` requests to the same broker and responds with OpenCode's one-shot `once` or `reject` decision. It never answers `always`. OpenCode applies its own project permissions and tool sandboxing; those provider-managed rules do not become Caelum filesystem restrictions.

Antigravity selects only ACP `allow_once` or `reject_once` options. If the
corresponding one-shot option is absent, it cancels the request.

## Child-process lifecycle

Provider adapters must launch a configured executable with a discrete argument vector, never by interpolating input into a shell command. Each child must be registered with `ProcessRegistry`. It can be polled for an exit status so the adapter can decide whether to recover, explicitly terminated on cancellation, and killed/reaped on app shutdown or when its managed handle is dropped.

The Codex adapter starts `codex app-server --listen stdio://` with an explicit argument vector and validated project working directory. The child is registered with `ProcessRegistry`; an unexpected exit fails outstanding requests and the next operation starts a fresh server. App shutdown terminates/reaps all registered provider children. The stdio transport avoids exposing an app-server network listener.

The OpenCode adapter first discovers the registered background service, accepts only a loopback endpoint, and verifies readiness, PID, and version through `/api/info`. Its optional registered password is used for Basic authentication. This externally owned service is never registered for termination by Caelum. The fallback starts `opencode serve --hostname 127.0.0.1 --port <available-port>` with the validated project as its working directory. OpenCode generates a random password by default; Caelum captures it from startup output and keeps it in memory for local Basic authentication. If `OPENCODE_SERVER_PASSWORD` is configured, Caelum reuses it instead. Provider credentials remain under OpenCode's control.

## Desktop launch and storage

`caelum://open?path=…` accepts a single absolute project path, then uses the same
core project validation as the CLI. It does not dispatch commands or prompts.
Workspace JSON is private, atomically replaced and locked for the desktop's
lifetime. Migration copies legacy data and preserves the sources. Credentials
remain provider-owned; they do not belong in workspace snapshots or branch metadata.

**Settings → Remote machines** authenticates a Boat key before saving it to macOS
Keychain or Linux Secret Service. Disconnect removes it. The adapter exposes
account-scoped machine metadata and process names, not desktop access URLs,
environment secrets or process arguments. Boat manages credentials and tool
permissions for its integrated Codex and OpenCode conversations.
Expanding a VM tool call reads its published event for that machine and
conversation. The selected call’s command and text output stay in memory;
the workspace saves only the summary and a reference for reopening it.

Opening a VM session authorizes a dedicated SSH public key for the selected
Boat account and machine. Each attachment uses only the host key from that
Boat response. The private key stays in the machine-scoped laptop profile.
Drafts stay in the laptop workspace, scoped to the owning VM checkout. The
desktop verifies the checkout’s physical Git root and origin over SSH. Native
sessions connect to Codex only through the forwarded loopback port. File operations resolve paths
using SFTP on the VM. The VM receives neither the Boat API key nor laptop provider credentials.
