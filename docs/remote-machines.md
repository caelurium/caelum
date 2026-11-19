# Run OpenCode on Boat

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

**Auto-stop** defaults to one hour. Boat renews the deadline while a managed
prompt is running or waiting. Trials allow up to two hours; on a paid account
you can also choose 8 hours, 24 hours or **Until stopped**. With **Until stopped**,
the VM continues billing until you stop it.

If **Create checkout environment** or **Configure checkout environment** returns
403, your Boat key needs `environment.write` for first-time repository setup.
Alternatively, configure a single-repository environment in Boat with GitHub
access and agent credentials enabled. See [Boat key scopes](https://docs.boat.dev/api-keys/scopes.md).

Send a message from the **Remote · machine-name** composer. **Stop** interrupts
that OpenCode conversation. Reopen Caelum or select the machine's sidebar row
to continue the same thread. If Boat may have accepted a prompt but the
acknowledgement was lost, check Boat and reconnect using the prompt ID.

Click the machine badge for capacity, auto-stop, and the busiest sampled
process. **Pin stats to composer** keeps its name and CPU visible when there's
room. Live samples update every five seconds while shown; historical samples
are labeled. **Stop machine** is separate from stopping a conversation.

Open **Changes** from the badge or **View remote changes** from the workspace
menu. Review and commit the VM's tracked and non-ignored new files; **Push
branch** publishes the reviewed commit to GitHub. In your local project, run
`git fetch origin` to bring the branch back.
