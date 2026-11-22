use super::{State, card, close, key, reveal};
use crate::{
    Workspace,
    context::EditMenu,
    live::{live, live_mut},
    quiet_button, ui,
};
use caelum_core::application::FeedSession;
use rune::{El, ScrollSpec, SurfaceFit, TextStyle, col, el, icon, row, text};
use rune_kit::{Kit, ScrollBar, glyphs};
use std::rc::Rc;

pub(crate) fn view(s: &Workspace, k: &Kit, state: &State) -> El {
    let l = live(s);
    if state.origin.get().is_none()
        && let Some(mark) = l.reading_bookmark()
    {
        let _ = state.origin.set(mark);
    }
    let size = rune::memo::window_layout("shell").unwrap_or([0.0, 0.0, 1280.0, 1000.0]);
    let compact = size[3] < 730.0;
    let rows = l.app.overview_sessions(&state.search.text);
    let count = rows.len();
    let mut stage = col()
        .key("conversation-overview")
        .role("dialog")
        .label("Conversation overview")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .items_center()
        .fill(k.c.bg_base)
        .modal(true, close)
        .modal_click_outside(false)
        .transition(rune::Transition::scale(1.025));
    if let Some(art) = &l.home.background {
        let image = if k.dark { &art.dark } else { &art.light };
        stage = stage.child(
            el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .pixels_fit(image.pixels(), SurfaceFit::Fill)
                .opacity(if k.dark { 0.27 } else { 0.33 })
                .a11y_hidden(),
        );
    }
    let header = row()
        .w_full()
        .center_y()
        .gap(12.0)
        .child(icon(&crate::ORBIT, if compact { 26.0 } else { 32.0 }).color(k.c.text_accent))
        .child(
            text("Active conversations")
                .font(TextStyle::new(if compact { 26.0 } else { 34.0 }, 550.0))
                .color(k.c.text_base)
                .grow(1.0)
                .min_w(0.0),
        )
        .child(
            quiet_button(&glyphs::CLOSE, "Return to workspace · Esc")
                .on_press(close)
                .build(k)
                .key("overview-close"),
        );
    let search = ui::field(
        &state.search,
        "Filter active conversations",
        "Filter by title, project or branch…",
        k,
    )
    .w(270.0)
    .max_w(300.0)
    .edit_menu_changed(
        "overview-search",
        |s| {
            live_mut(s)
                .panels
                .conversations_mut()
                .map(|v| &mut v.search)
        },
        |s, tx| {
            if let Some(state) = live_mut(s).panels.conversations_mut() {
                state.cursor = None;
                tx.scroll_to_y("overview-scroll", 0.0);
            }
        },
    )
    .on_input_key(key);
    let waiting = if l.app.questions.is_empty() && l.app.approvals.is_empty() {
        0
    } else {
        rows.iter().filter(|id| super::needs_input(l, id)).count()
    };
    let toolbar = row()
        .w_full()
        .center_y()
        .gap(12.0)
        .child(ui::caption(
            format!(
                "All open projects  /  {count} {}",
                if count == 1 { "session" } else { "sessions" }
            ),
            k,
        ))
        .children((waiting > 0).then(|| {
            let label = format!(
                "{waiting} {} input",
                if waiting == 1 { "needs" } else { "need" }
            );
            k.interactive(
                row()
                    .center_y()
                    .gap(5.0)
                    .h(26.0)
                    .px(8.0)
                    .radius(6.0)
                    .fill(k.alpha(k.c.warning.fg, 0.09))
                    .child(icon(&ui::BELL, 12.0).color(k.c.warning.fg))
                    .child(text(&label).font(k.ty.small_medium).color(k.c.warning.fg)),
            )
            .key("overview-requests")
            .role("button")
            .label(label)
            .help("Highlight the next conversation that needs your input")
            .focusable(true)
            .focus_ring(1.5, 0.0, k.c.border_focus)
            .on_press_tx(super::next_request)
        }))
        .child(el().grow(1.0))
        .child(search);
    let body = if rows.is_empty() {
        let (title, detail) = if state.search.text.trim().is_empty() {
            (
                "No active conversations",
                "Conversations appear here while work is running, queued or waiting for your input.",
            )
        } else {
            (
                "No matching active conversations",
                "Try a different title, project or branch.",
            )
        };
        col()
            .w_full()
            .grow(1.0)
            .center()
            .child(ui::empty(title, detail, k))
    } else {
        grid(rows, state, compact, k)
    };
    stage.child(
        col()
            .w_full()
            .max_w(1180.0)
            .h_full()
            .px(if compact { 28.0 } else { 48.0 })
            .pt(if compact { 48.0 } else { 80.0 })
            .pb(if compact { 14.0 } else { 30.0 })
            .gap(if compact { 12.0 } else { 20.0 })
            .child(header)
            .child(toolbar)
            .child(body)
            .child(ui::caption(
                "Arrow keys to browse   ·   Return to open   ·   Esc to go back",
                k,
            )),
    )
}

fn grid(rows: Vec<FeedSession>, state: &State, compact: bool, k: &Kit) -> El {
    let rows = Rc::new(rows);
    let columns = state.columns.max(1);
    let selected = state
        .cursor
        .as_ref()
        .filter(|id| rows.contains(id))
        .or(rows.first())
        .cloned();
    let height = if compact { 136.0 } else { 174.0 };
    let kit = k.clone();
    let items = rows.clone();
    let list = col()
        .w_full()
        .key("overview-rows")
        .growth(rune::GrowthSpec::default())
        .virtual_rows(
            items.len().div_ceil(columns),
            height,
            |_: &Workspace, row| format!("overview-row-{row}").into(),
            move |s: &Workspace, i| {
                let chunk = &items[i * columns..((i + 1) * columns).min(items.len())];
                let mut line = row()
                    .key(format!("overview-row-{i}"))
                    .w_full()
                    .h(height)
                    .pb(10.0)
                    .gap(12.0);
                for col_index in 0..columns {
                    let tile = chunk
                        .get(col_index)
                        .map(|id| card::tile(s, id, selected.as_ref() == Some(id), compact, &kit));
                    line = line.child(col().grow(1.0).min_w(0.0).h_full().children(tile));
                }
                line
            },
        )
        .virtual_rows_version((columns, rows.as_ref()));
    col()
        .key("overview-scroll")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(list)
        .scroll_bar(k)
        .on_layout_tx(move |s: &mut Workspace, rect, tx| {
            let columns = if rect[2] >= 920.0 {
                3
            } else if rect[2] >= 580.0 {
                2
            } else {
                1
            };
            if let Some(state) = live_mut(s).panels.conversations_mut()
                && state.columns != columns
            {
                state.columns = columns;
                if let Some(index) = state
                    .cursor
                    .as_ref()
                    .and_then(|id| rows.iter().position(|r| r == id))
                {
                    reveal(tx, index, columns);
                }
            }
        })
}
