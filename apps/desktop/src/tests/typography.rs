use super::*;
use rune::{Span, TextStyle, text::TextSystem};

#[test]
fn native_font_selection_keeps_faces_wrapping_and_monospace_fallbacks() {
    let app = spec();
    // Exercise the app's real family list at Retina scale, including fonts that
    // a generic-only list reaches through Rune's symbol/script fallbacks.
    let _fonts = rune::text::with_families(app.families.0, app.families.1);
    let mut previous = TextSystem::new(&[], ("system-ui", "ui-monospace"));
    let mut current = TextSystem::new(&[], app.families);
    let source = "Keep café drafts **independent**; 日本語 and emoji 🎉. Copy ⌘K. let owner = session.key();";
    let code = source.find("let owner").unwrap();
    let spans = [Span::new(code, source.len()).mono()];
    for scale in [1.0, 2.0, 3.0] {
        for size in [11.0, 14.0, 15.0, 22.0] {
            for weight in [400.0, 550.0, 600.0] {
                for italic in [false, true] {
                    let mut style = TextStyle::new(size, weight).lh(size * 1.7);
                    if italic {
                        style = style.italic();
                    }
                    let old =
                        previous.layout_rich(source, &spans, style, scale, Some(320.0 * scale));
                    let new =
                        current.layout_rich(source, &spans, style, scale, Some(320.0 * scale));
                    let context = format!("{scale}x {size}pt {weight} italic={italic}");
                    assert_eq!(old.len(), new.len(), "{context}");
                    assert!(
                        (old.full_width() - new.full_width()).abs() < 0.01,
                        "{context}: width {} -> {}",
                        old.full_width(),
                        new.full_width()
                    );
                    assert!(
                        (old.height() - new.height()).abs() < 0.01,
                        "{context}: height"
                    );
                    for (a, b) in old.lines().zip(new.lines()) {
                        assert_eq!(a.text_range(), b.text_range(), "{context}");
                        assert!(
                            (a.metrics().advance - b.metrics().advance).abs() < 0.01,
                            "{context}: line advance"
                        );
                        let a: Vec<_> = a.runs().collect();
                        let b: Vec<_> = b.runs().collect();
                        assert_eq!(a.len(), b.len(), "{context}");
                        for (a, b) in a.iter().zip(&b) {
                            assert_eq!(a.text_range(), b.text_range(), "{context}");
                            assert_eq!(a.font().index, b.font().index, "{context}");
                            assert!(
                                a.font().data.as_ref() == b.font().data.as_ref(),
                                "{context}: font face"
                            );
                            // Resolving SF before shaping also supplies its named
                            // italic weight's hidden axes. Upright and monospace
                            // instances are unchanged; all line geometry above
                            // must still agree, including italic text.
                            if !italic || a.text_range().start >= code {
                                assert_eq!(
                                    a.normalized_coords(),
                                    b.normalized_coords(),
                                    "{context}: font instance"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
