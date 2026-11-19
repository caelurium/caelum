//! Nested checkout navigation shares the same rail, disclosure and row metrics.
use rune::{El, Icon, TextStyle, col, icon, row, text};
use rune_kit::Kit;

pub(crate) fn group(k: &Kit) -> El {
    col()
        .w_full()
        .gap(2.0)
        .pl(16.0)
        .pr(6.0)
        .pb(6.0)
        .role("group")
        .child(
            rune::el()
                .w(1.0)
                .fill(k.c.border_muted)
                .pin(Some(9.0), Some(5.0), None, Some(12.0)),
        )
}

pub(crate) fn toggle(title: &str, count: usize, expanded: bool, k: &Kit) -> El {
    k.interactive(
        row()
            .h(28.0)
            .grow(1.0)
            .min_w(0.0)
            .gap(5.0)
            .center_y()
            .radius(5.0)
            .px(4.0)
            .child(super::disclosure(expanded, k))
            .child(
                text(title)
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .truncate(),
            )
            .child(
                text(count.to_string())
                    .font(k.ty.small)
                    .color(k.c.text_faint),
            ),
    )
    .role("button")
    .expanded(expanded)
    .focusable(true)
    .focus_ring(1.0, 0.0, k.c.border_focus)
}

pub(crate) fn item(title: &str, detail: &str, glyph: &'static Icon, selected: bool, k: &Kit) -> El {
    k.interactive(
        row()
            .w_full()
            .h(42.0)
            .px(8.0)
            .gap(7.0)
            .center_y()
            .radius(6.0)
            .fill(if selected {
                k.c.hover
            } else {
                rune::Color::TRANSPARENT
            })
            .child(icon(glyph, 12.0).color(if selected {
                k.c.icon_accent
            } else {
                k.c.icon_muted
            }))
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(2.0)
                    .child(text(title).font(k.ty.ui).color(k.c.text_base).truncate())
                    .child(
                        text(detail)
                            .font(TextStyle::new(10.0, 400.0))
                            .color(k.c.text_muted)
                            .truncate(),
                    ),
            )
            .children(selected.then(|| icon(k.glyph.check_small, 12.0).color(k.c.icon_accent))),
    )
    .role("button")
    .focusable(true)
    .focus_selected(selected)
    .focus_ring(1.0, 0.0, k.c.border_focus)
}
