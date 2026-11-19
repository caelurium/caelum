# Run OpenCode on Boat

A remote machine has its own Git checkout and disk. Caelum sends messages to
[Boat’s integrated OpenCode harness](https://docs.boat.dev/platform-guide/agents.md).
Set the OpenCode model and credentials in [Boat → Agents & Proxying](https://boat.dev/dashboard?tab=agents).
Caelum uses that Boat default on every remote prompt; the local model, reasoning
effort and Build/Plan/custom-agent selections do not transfer. There is no
Caelum agent or OpenCode server to install on the VM.

## Create or attach a checkout

1. Connect GitHub and an OpenCode-compatible model credential in your
   [Boat account](https://docs.boat.dev/integrated-agents/keys.md). In Caelum,
   open **Settings → Remote machines**, enter your Boat API key and click
   **Connect Boat**. The key is stored in Keychain or Linux Secret Service.
2. Click **Create VM…** in **Settings → Remote machines** or **New session**.
   Use the current project, choose another, or open a project folder with a GitHub `origin`.
3. Choose a published base (defaults to the origin default branch, or `main`),
   type a **new branch** name, choose size and auto-stop, and click **Create VM**.
   Caelum provisions the repository using a compatible single-repo
   environment or creates a dedicated one. Unpublished commits, staged changes
   and files on your laptop stay there.

To attach instead, right-click a project, choose **Open on remote machine…**,
and select an existing machine. It must have a
current single-repo environment for the project's GitHub origin. **Resume** a
stopped machine to use its saved disk; attaching keeps its current branch.
If setup fails, **Refresh checkout** retries that checkout on the same machine.

### Connected, but Create VM returns 403

Connecting checks that the key can read the account and list VMs. Creating a VM
needs `sandbox.create` for its environment. **First-time repository setup also
needs `environment.write`** to create and configure the Boat environment.

- If Caelum reports **Create checkout environment** or **Configure checkout
  environment** and your key lacks `environment.write`, create a scoped key
  with your current permissions plus `environment.write` in [Boat → API Keys](https://boat.dev/dashboard?tab=api-keys).
  Enter it in **Settings → Remote machines → Connect Boat** for the same
  account. Reopen **one** failed checkout, or click **Refresh checkout** if it’s
  already open, to retry its original request.
- To keep a key without `environment.write`, configure an [environment in Boat](https://docs.boat.dev/environments/repositories.md)
  with just this repository, GitHub access and agent credentials enabled, and
  **Safe for third parties** off. Reopen VM setup so Caelum discovers it. The
  key still needs `environment.read` and `sandbox.create` for that environment,
  `exec` to prepare the checkout, and `agent.prompt` to send a prompt.

If the failing step is **Create VM**, check the action **and** the key’s allowed
environments, or follow the size restriction in the error. See [Boat key scopes](https://docs.boat.dev/api-keys/scopes.md).

## Continue a local session on a VM

Click the **server icon** beside **Context** in a local Codex or OpenCode composer.
On **Home** it’s beside **Send**. The picker opens at the icon; on Home, choose the
Home draft or a recent session. Finish any active local turn and queued prompts first.

For a handoff, **Session branch** is the current branch of that session’s exact
worktree (or Home’s chosen checkout). A new VM opens **that same branch** at
its published commit on `origin`. The name is shown for reference; there’s
nothing to enter. An existing VM keeps its branch and must already contain the
published commit.

Choose an existing VM, or choose the size and auto-stop time and click
**Create VM**. The remote composer opens with a copy of your unsent draft.
Your original session and draft stay on the laptop.
If the VM already had a conversation, **Conversations…** switches back to it.

The first time you press **Send**, Caelum includes the local user and assistant
messages as context for a new OpenCode conversation. Setup shows how much chat
will travel: up to the most recent 200 messages or 64 KiB of text. Tool output,
reasoning, attachments and unpublished Git work stay local.

## Work on the VM

Send a message from the remote composer. **Stop** interrupts
that OpenCode conversation. Reopen Caelum or select the machine's sidebar row
to continue the same thread. If Boat may have accepted a prompt but the
acknowledgement was lost, check Boat and reconnect using the prompt ID.

Click the server icon in the remote composer or the original linked session for
machine stats: capacity, the auto-stop deadline, and the busiest sampled process.
From the original session, **Open VM conversation** returns to its specific
thread. **Pin stats to VM composer** keeps process activity visible on the VM.

The default lifetime is **1 h**. For longer work, choose **8 h**, **24 h**,
or **Until stopped**. Boat automatically moves a timed VM’s deadline forward
while an OpenCode prompt is running or waiting, even if Caelum is closed.
Standalone commands and services don’t renew it. In machine stats, **Set from
now** changes the remaining time without interrupting the turn. **Until stopped**
keeps billing until you stop the VM; trials require a timer of at most 2 h.
See [Boat’s auto-stop guide](https://docs.boat.dev/long-running-tasks/auto-stop.md).

Open **Changes** from machine stats on the VM or **View remote changes** from the workspace
menu. Review and commit the VM's tracked and non-ignored new files; **Push
branch** publishes the reviewed commit to GitHub. In your local project, run
`git fetch origin` to bring the branch back.
