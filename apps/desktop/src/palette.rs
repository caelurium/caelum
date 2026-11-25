//! Global command palette (`⌘K`) for jumping to sessions, projects, worktrees, settings, and actions.
mod execute;

use execute::execute;

use crate::{
    Workspace,
    context::EditMenu,
    live::{live, live_mut},
    settings::Page as SettingsPage,
    ui,
};
use caelum_core::application::Provider;
use rune::{El, Key, KeyEvent, ScrollSpec, TextEdit, Tx, col, row, text};
use rune_kit::{Kit, ScrollBar};

pub struct State {
    pub query: TextEdit,
    pub active: usize,
}

impl State {
    pub fn new() -> Self {
        let mut query = TextEdit::new("");
        query.focused = true;
        Self { query, active: 0 }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Commands,
    Sessions,
    Projects,
    Settings,
}

impl Section {
    const fn label(self) -> &'static str {
        match self {
            Self::Commands => "Commands",
            Self::Sessions => "Sessions",
            Self::Projects => "Projects & Worktrees",
            Self::Settings => "Settings",
        }
    }
}

#[derive(Clone, Copy)]
enum BadgeTone {
    Accent,
    Muted,
}

#[derive(Clone)]
enum Action {
    NewSession,
    Launcher,
    Home,
    Overview,
    Settings(SettingsPage),
    Worktrees,
    ToggleSidebar,
    ToggleInspector,
    Files,
    Changes,
    Runner,
    SplitRight,
    CloseSplit,
    OpenProject,
    ChooseModel,
    Timeline,
    Usage,
    Stats,
    ProviderSessions,
    Fork,
    Rename,
    SideQuestion,
    SelectSession(usize),
    SelectProject(String),
    SelectWorktree(String),
}

struct Entry {
    section: Section,
    title: String,
    detail: String,
    shortcut: Option<&'static str>,
    badge: Option<(&'static str, BadgeTone)>,
    search: String,
    action: Action,
}

impl Entry {
    fn cmd(
        title: &'static str,
        detail: &'static str,
        shortcut: Option<&'static str>,
        keywords: &'static str,
        action: Action,
    ) -> Self {
        Self::new(
            Section::Commands,
            title,
            detail,
            shortcut,
            None,
            keywords,
            action,
        )
    }

    fn new(
        section: Section,
        title: impl Into<String>,
        detail: impl Into<String>,
        shortcut: Option<&'static str>,
        badge: Option<(&'static str, BadgeTone)>,
        keywords: &str,
        action: Action,
    ) -> Self {
        let title = title.into();
        let detail = detail.into();
        let search = format!("{} {} {}", title, detail, keywords).to_lowercase();
        Self {
            section,
            title,
            detail,
            shortcut,
            badge,
            search,
            action,
        }
    }
}

pub fn open(s: &mut Workspace) {
    let l = live_mut(s);
    l.session_menu = None;
    l.panels.open_palette(State::new());
}

fn build_entries(s: &Workspace, query: &str) -> Vec<Entry> {
    let l = live(s);
    let trimmed = query.trim().to_lowercase();
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    let filtering = !tokens.is_empty();
    let mut entries = Vec::new();

    let active_session = l.app.selected.and_then(|i| l.app.sessions.get(i));
    let has_project = l.app.active_project.is_some();
    let provider = active_session.map(|sess| sess.scope.provider);
    let provider_available = active_session.is_some_and(|sess| sess.scope.provider_available());
    let has_remote_session = active_session.is_some_and(|sess| sess.remote_id.is_some());

    // Commands
    entries.push(Entry::new(
        Section::Commands,
        "New session",
        "Start a fresh conversation in the current project",
        Some("⌘N"),
        None,
        "create chat conversation draft",
        Action::NewSession,
    ));
    entries.push(Entry::new(
        Section::Commands,
        "New session launcher",
        "Choose a local project, worktree, or remote VM",
        Some("⇧⌘N"),
        None,
        "start checkout vm worktree",
        Action::Launcher,
    ));
    entries.extend([
        Entry::cmd(
            "Home",
            "Open the home composer and project overview",
            None,
            "start dashboard",
            Action::Home,
        ),
        Entry::cmd(
            "Overview",
            "Dashboard of sessions, pull requests, worktrees, machines, and usage",
            Some("⌘J"),
            "overview active conversations switcher dashboard",
            Action::Overview,
        ),
        Entry::cmd(
            "Open project…",
            "Add a local Git repository or folder to Caelum",
            Some("⌘O"),
            "folder repository add",
            Action::OpenProject,
        ),
    ]);
    if has_project {
        entries.extend([
            Entry::cmd(
                "Manage worktrees",
                "Create, rename, or remove Caelum worktrees",
                None,
                "git branch checkout",
                Action::Worktrees,
            ),
            Entry::cmd(
                "Project files",
                "Browse and search files in the current project",
                Some("⌘P"),
                "explorer tree search",
                Action::Files,
            ),
            Entry::cmd(
                "Git changes",
                "Inspect modified files and diffs in the inspector",
                None,
                "diff status review",
                Action::Changes,
            ),
        ]);
        if crate::runner::active_project(s).is_some() {
            entries.push(Entry::cmd(
                "Run checkout command",
                "Run a shell command in this checkout and attach output",
                None,
                "shell terminal test build runner",
                Action::Runner,
            ));
        }
    }
    if l.app.selected.is_some() {
        entries.push(Entry::cmd(
            "Split right",
            "View a session or PR review side-by-side",
            Some("⌥⌘→"),
            "split pane two columns",
            Action::SplitRight,
        ));
    }
    if l.split.is_open() {
        entries.push(Entry::cmd(
            "Close split pane",
            "Return to single-pane layout",
            None,
            "unsplit single pane",
            Action::CloseSplit,
        ));
    }
    if (active_session.is_some() && provider_available) || l.app.remote.saved.selected.is_some() {
        entries.push(Entry::new(
            Section::Commands,
            "Statistics",
            "Inspect token usage, cost, and activity history",
            None,
            None,
            "tokens cost metrics stats",
            Action::Stats,
        ));
    }
    if active_session.is_some() && provider_available {
        entries.push(Entry::new(
            Section::Commands,
            "Choose model",
            "Switch model or reasoning effort for this session",
            None,
            None,
            "llm reasoning effort variant",
            Action::ChooseModel,
        ));
        entries.push(Entry::new(
            Section::Commands,
            "Timeline",
            "Jump to an earlier prompt in this conversation",
            None,
            None,
            "history prompts scroll",
            Action::Timeline,
        ));
        entries.push(Entry::new(
            Section::Commands,
            "Provider sessions",
            "Browse and resume earlier conversations from this provider",
            None,
            None,
            "resume import history",
            Action::ProviderSessions,
        ));
        entries.push(Entry::new(
            Section::Commands,
            "Fork conversation",
            "Branch a new conversation from an earlier message",
            None,
            None,
            "branch split",
            Action::Fork,
        ));
        entries.push(Entry::new(
            Section::Commands,
            "Rename session",
            "Set a custom title saved with the provider",
            None,
            None,
            "title label",
            Action::Rename,
        ));
        if matches!(provider, Some(Provider::OpenCode | Provider::Antigravity)) {
            entries.push(Entry::new(
                Section::Commands,
                "Usage",
                "Check provider subscription allowances and reset windows",
                None,
                None,
                "quota limits allowance",
                Action::Usage,
            ));
        }
        if has_remote_session {
            entries.push(Entry::new(
                Section::Commands,
                "Side question",
                "Ask a quick side question without altering the main thread",
                None,
                None,
                "btw ask aside",
                Action::SideQuestion,
            ));
        }
    }
    entries.push(Entry::new(
        Section::Commands,
        if s.sidebar.is_open() {
            "Hide sidebar"
        } else {
            "Show sidebar"
        },
        "Toggle the left project and session sidebar",
        Some("⌘\\"),
        None,
        "toggle sidebar navigation",
        Action::ToggleSidebar,
    ));
    entries.push(Entry::new(
        Section::Commands,
        if s.inspector.is_open() {
            "Hide inspector"
        } else {
            "Show inspector"
        },
        "Toggle the right files and changes inspector",
        Some("⌥⌘I"),
        None,
        "toggle inspector panel",
        Action::ToggleInspector,
    ));
    entries.push(Entry::new(
        Section::Commands,
        "Settings",
        "Open workspace preferences and provider configuration",
        Some("⌘,"),
        None,
        "preferences config options",
        Action::Settings(l.panels.settings_page),
    ));

    // Sessions (most recent first)
    let mut session_indices: Vec<usize> = l
        .app
        .sessions
        .iter()
        .enumerate()
        .filter(|(_, sess)| !sess.forgotten() && (filtering || !sess.archived()))
        .map(|(idx, _)| idx)
        .collect();
    session_indices.sort_by(|&a, &b| {
        l.app.sessions[b]
            .last_activity
            .cmp(&l.app.sessions[a].last_activity)
            .then_with(|| b.cmp(&a))
    });
    let session_limit = if filtering { 24 } else { 8 };
    for idx in session_indices.into_iter().take(session_limit) {
        let sess = &l.app.sessions[idx];
        let title = if sess.title.trim().is_empty() {
            "Untitled session"
        } else {
            sess.title.as_str()
        };
        let project_name = sess.scope.project.rsplit('/').next().unwrap_or("Project");
        let mut meta = vec![project_name.to_owned()];
        if let Some(branch) = l.app.session_branch(sess) {
            meta.push(branch.to_owned());
        }
        meta.push(sess.scope.provider.name().to_owned());
        let badge = if sess.working() {
            Some(("Working", BadgeTone::Accent))
        } else if sess.archived() {
            Some(("Archived", BadgeTone::Muted))
        } else if l.app.selected == Some(idx) {
            Some(("Current", BadgeTone::Muted))
        } else {
            None
        };
        entries.push(Entry::new(
            Section::Sessions,
            title,
            meta.join(" · "),
            None,
            badge,
            &format!("{} {}", sess.scope.project, sess.scope.provider.as_str()),
            Action::SelectSession(idx),
        ));
    }

    // Projects & Worktrees
    for project in &l.app.projects {
        if l.app.group_path(&project.path) != project.path {
            continue;
        }
        let is_active = l.app.active_project.as_deref() == Some(project.path.as_str());
        entries.push(Entry::new(
            Section::Projects,
            &project.name,
            &project.path,
            None,
            is_active.then_some(("Active", BadgeTone::Muted)),
            "project repository",
            Action::SelectProject(project.path.clone()),
        ));
        for tree in l.app.managed_worktrees(&project.path) {
            let tree_name = tree.path.rsplit('/').next().unwrap_or(&tree.path);
            let branch = tree.branch.as_deref().unwrap_or("detached");
            let is_active_tree = l.app.active_project.as_deref() == Some(tree.path.as_str());
            entries.push(Entry::new(
                Section::Projects,
                format!("{} / {}", project.name, tree_name),
                format!("Worktree · {branch}"),
                None,
                is_active_tree.then_some(("Active", BadgeTone::Muted)),
                &format!("{} {} worktree", tree.path, branch),
                Action::SelectWorktree(tree.path.clone()),
            ));
        }
    }

    // Settings pages
    let pages = [
        (
            SettingsPage::Appearance,
            "Settings: Appearance",
            "Theme, density, and visual preferences",
            "theme dark light font ui",
        ),
        (
            SettingsPage::Providers,
            "Settings: Providers",
            "Configure Codex, OpenCode, and Antigravity providers",
            "codex opencode antigravity api",
        ),
        (
            SettingsPage::Remote,
            "Settings: Remote machines",
            "Manage Boat VMs and remote SSH checkouts",
            "boat vm ssh cloud",
        ),
        (
            SettingsPage::Worktrees,
            "Settings: Worktrees",
            "Configure default worktree directory",
            "git checkout folder path",
        ),
        (
            SettingsPage::Notifications,
            "Settings: Notifications",
            "Desktop alerts when sessions finish or need input",
            "alerts sound badges",
        ),
        (
            SettingsPage::About,
            "Settings: About",
            "Version, updates, and build information",
            "version update info",
        ),
    ];
    for (page, title, detail, keywords) in pages {
        entries.push(Entry::new(
            Section::Settings,
            title,
            detail,
            None,
            None,
            keywords,
            Action::Settings(page),
        ));
    }

    if !filtering {
        return entries;
    }

    entries.retain(|entry| tokens.iter().all(|tok| entry.search.contains(tok)));
    entries.sort_by_key(|entry| {
        let lower_title = entry.title.to_lowercase();
        let rank = if lower_title == trimmed {
            0
        } else if lower_title.starts_with(&trimmed) {
            1
        } else if lower_title.contains(&trimmed) {
            2
        } else {
            3
        };
        (rank, entry.section as usize)
    });
    entries
}

pub fn on_key(s: &mut Workspace, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let Some(state) = live(s).panels.palette() else {
        return false;
    };
    let entries = build_entries(s, &state.query.text);
    let count = entries.len();
    let current = state.active.min(count.saturating_sub(1));
    match ev.key {
        Key::Down => {
            if count > 0 {
                let next = (current + 1) % count;
                if let Some(p) = live_mut(s).panels.palette_mut() {
                    p.active = next;
                    p.query.focused = true;
                }
                tx.scroll_center_in_instant("palette-scroll", format!("palette-item-{next}"));
            }
            true
        }
        Key::Up => {
            if count > 0 {
                let next = if current == 0 { count - 1 } else { current - 1 };
                if let Some(p) = live_mut(s).panels.palette_mut() {
                    p.active = next;
                    p.query.focused = true;
                }
                tx.scroll_center_in_instant("palette-scroll", format!("palette-item-{next}"));
            }
            true
        }
        Key::Enter => {
            if let Some(entry) = entries.into_iter().nth(current) {
                execute(s, tx, entry.action);
            }
            true
        }
        _ => false,
    }
}

pub fn view(s: &Workspace, k: &Kit) -> El {
    let Some(state) = live(s).panels.palette() else {
        return col();
    };
    let entries = build_entries(s, &state.query.text);
    let count = entries.len();
    let active = state.active.min(count.saturating_sub(1));

    let mut list = col()
        .w_full()
        .gap(2.0)
        .key("palette-list")
        .role("list")
        .focus_group();
    let mut last_section: Option<Section> = None;
    for (idx, entry) in entries.into_iter().enumerate() {
        if last_section != Some(entry.section) {
            list = list.child(
                text(entry.section.label())
                    .font(k.ty.small)
                    .color(k.c.text_faint)
                    .px(10.0)
                    .pt(if last_section.is_some() { 8.0 } else { 2.0 })
                    .pb(2.0),
            );
            last_section = Some(entry.section);
        }
        let selected = idx == active;
        let action = entry.action;
        let badge = entry.badge.map(|(label, tone)| {
            let color = match tone {
                BadgeTone::Accent => k.c.text_accent,
                BadgeTone::Muted => k.c.text_muted,
            };
            text(label)
                .font(k.ty.small)
                .color(color)
                .px(6.0)
                .py(1.0)
                .radius(4.0)
                .fill(k.c.layer_02)
        });
        let shortcut = entry.shortcut.map(|sc| {
            text(ui::shortcut(sc))
                .font(k.ty.small)
                .color(k.c.text_muted)
                .px(6.0)
                .py(1.0)
                .radius(4.0)
                .fill(k.c.layer_02)
                .border(1.0, k.c.border_base)
                .a11y_hidden()
        });
        let row_el = k
            .interactive(
                row()
                    .w_full()
                    .min_h(42.0)
                    .px(10.0)
                    .py(6.0)
                    .gap(10.0)
                    .center_y()
                    .radius(6.0)
                    .fill(if selected {
                        k.c.hover
                    } else {
                        rune::Color::TRANSPARENT
                    })
                    .child(
                        col()
                            .grow(1.0)
                            .min_w(0.0)
                            .gap(1.0)
                            .child(
                                row()
                                    .w_full()
                                    .center_y()
                                    .gap(8.0)
                                    .child(
                                        text(&entry.title)
                                            .font(k.ty.ui_medium)
                                            .color(k.c.text_base)
                                            .truncate()
                                            .grow(1.0)
                                            .min_w(0.0),
                                    )
                                    .children(badge),
                            )
                            .child(ui::caption(&entry.detail, k).truncate()),
                    )
                    .children(shortcut)
                    .child(
                        text("↵")
                            .font(k.ty.small)
                            .color(k.c.text_muted)
                            .opacity(if selected { 1.0 } else { 0.0 })
                            .a11y_hidden(),
                    ),
            )
            .key(format!("palette-item-{idx}"))
            .role("button")
            .label(&entry.title)
            .description(&entry.detail)
            .focusable(true)
            .focus_ring(1.5, 0.0, k.c.border_focus)
            .on_press_tx(move |s: &mut Workspace, tx: &mut Tx| execute(s, tx, action.clone()));
        list = list.child(row_el);
    }

    let mut body = col()
        .w_full()
        .h_full()
        .min_h(0.0)
        .gap(10.0)
        .child(
            ui::field(
                &state.query,
                "Command palette search",
                "Jump to a session, project, worktree, or command…",
                k,
            )
            .w_full()
            .single_line()
            .edit_menu_changed(
                "palette-search",
                |s| live_mut(s).panels.palette_mut().map(|p| &mut p.query),
                |s, tx| {
                    if let Some(p) = live_mut(s).panels.palette_mut() {
                        p.active = 0;
                    }
                    tx.scroll_to_instant("palette-scroll", 0.0, 0.0);
                },
            )
            .on_input_key(on_key),
        )
        .child(
            col()
                .key("palette-scroll")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(list)
                .scroll_bar(k),
        );

    if count == 0 {
        body = body.child(ui::empty(
            "No matching results",
            "Try a different query or press Esc to close.",
            k,
        ));
    }

    body.child(ui::divider(k)).child(
        row()
            .w_full()
            .center_y()
            .justify_between()
            .gap(8.0)
            .child(ui::caption("↑↓ Navigate · ↵ Open · Esc Close", k))
            .child(ui::caption(
                format!("{count} {}", if count == 1 { "result" } else { "results" }),
                k,
            )),
    )
}
