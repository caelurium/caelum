use caelum_core::application::FeedSession;
use rune::{Color, El, col, row, text};
use rune_kit::Kit;

pub(super) const HEIGHT: f32 = 88.0;

pub(super) fn key(session: &FeedSession) -> String {
    match session {
        FeedSession::Local(i) => format!("live-session-{i}"),
        FeedSession::Vm(link) => format!("vm-session-{}/{}", link.checkout, link.thread),
    }
}

pub(super) struct Card<'a> {
    pub badge: El,
    pub project: &'a str,
    pub title: &'a str,
    pub provider: &'a str,
    pub age: &'a str,
    pub ink: Color,
    pub mark: Option<El>,
}

impl Card<'_> {
    pub fn build(self, k: &Kit, bottom: El, selected: bool, position: usize, count: usize) -> El {
        k.interactive(
            col()
                .w_full()
                .h(HEIGHT - 4.0)
                .px(10.)
                .py(8.)
                .gap(3.)
                .hover_group()
                .child(
                    row()
                        .w_full()
                        .gap(5.)
                        .center_y()
                        .child(self.badge)
                        .child(
                            text(self.project)
                                .font(k.ty.small)
                                .color(k.c.text_muted)
                                .truncate()
                                .min_w(0.)
                                .grow(1.),
                        )
                        .children(self.mark)
                        .child(
                            text(self.provider)
                                .font(rune::TextStyle::new(10., 400.))
                                .color(self.ink),
                        )
                        .child(
                            text(self.age)
                                .key("session-age")
                                .font(k.ty.small)
                                .color(k.c.text_faint),
                        ),
                )
                .child(
                    text(self.title)
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base)
                        .max_lines(1)
                        .w_full(),
                )
                .child(bottom),
        )
        .role("listitem")
        .label(self.title)
        .focusable(true)
        .focus_selected(selected)
        .focus_group()
        .focus_ring(1., 0., k.c.border_focus)
        .position_in_set(position + 1, count)
    }
}
