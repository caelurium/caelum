use super::*;
use rune::{
    Compositor as _, Env, Runtime,
    render::{Gpu, OFFSCREEN_FORMAT, Offscreen, Renderer},
};

const PIXEL_SIZE: (u32, u32) = (1600, 1240);

#[test]
fn supplied_grid_keeps_its_spacing_colours_and_bounded_atlas() {
    let rows: Vec<_> = SOURCE.lines().collect();
    assert!(rows.len() <= HEIGHT);
    assert_eq!(rows.iter().map(|row| row.len()).max(), Some(WIDTH));
    assert!(
        rows.iter()
            .all(|row| row.bytes().all(|g| GLYPHS.contains(&g)))
    );
    assert_eq!(COLOURS.len(), WIDTH * HEIGHT * 3);
    assert_eq!(ATLAS.len(), 160 * 28);
    let cells = cells();
    assert_eq!(cells.len(), WIDTH * HEIGHT * 4);
    for (y, row) in rows.iter().enumerate() {
        for (x, glyph) in row.bytes().enumerate() {
            let i = y * WIDTH + x;
            let cell = &cells[i * 4..i * 4 + 4];
            if glyph == b' ' {
                assert_eq!(cell, [0, 0, 0, 0]);
            } else {
                assert_eq!(GLYPHS[cell[3] as usize], glyph);
                assert_eq!(&cell[..3], &COLOURS[i * 3..i * 3 + 3]);
            }
        }
    }
    // The palette is spatial, not a global monochrome tint.
    assert!(
        cells
            .chunks_exact(4)
            .any(|c| c[2] > c[0].saturating_add(30))
    );
    assert!(cells.chunks_exact(4).any(|c| c[0] > 140 && c[2] > 140));
    assert!(cells.len() + ATLAS.len() < 128 * 1024);
}

#[test]
fn motion_stops_at_rest_and_does_not_jump_after_a_hidden_window() {
    let mut motion = Motion::default();
    assert_eq!(motion.sample(1.0, true), 1.0);
    assert!((motion.sample(1.05, true) - 1.05).abs() < 0.0001);
    assert!((motion.sample(90.0, true) - 1.15).abs() < 0.0001);
    assert_eq!(motion.sample(91.0, false), 0.0);
    assert!(matches!(motion, Motion::Rest));
    assert_eq!(motion.sample(92.0, false), 0.0);
}

struct Draw {
    gpu: Gpu,
    renderer: Renderer,
    target: Offscreen,
    compositor: Compositor,
}

impl Draw {
    fn new() -> Self {
        let gpu = Gpu::headless();
        let renderer = Renderer::new(&gpu.device, OFFSCREEN_FORMAT);
        let target = Offscreen::new(&gpu.device, PIXEL_SIZE.0, PIXEL_SIZE.1);
        Self {
            gpu,
            renderer,
            target,
            compositor: Compositor::default(),
        }
    }

    fn frame(&mut self, rt: &mut Runtime<Workspace>) -> Vec<u8> {
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        self.renderer
            .upload_images(&self.gpu.device, &self.gpu.queue, &mut rt.images);
        self.compositor.render(&mut Frame {
            device: &self.gpu.device,
            queue: &self.gpu.queue,
            encoder: &mut encoder,
            target: &self.target.view,
            format: OFFSCREEN_FORMAT,
            size: PIXEL_SIZE,
            dt: 1.0 / 60.0,
            update_ms: 0.0,
            rt,
            ui: &mut self.renderer,
        });
        self.target.copy(&mut encoder);
        self.renderer.submit(&self.gpu.queue, encoder);
        self.target.read(&self.gpu.device)
    }
}

fn advance(rt: &mut Runtime<Workspace>, seconds: f64) {
    for _ in 0..(seconds * 60.0) as usize {
        rt.frame(1.0 / 60.0);
    }
}

#[test]
fn gpu_home_animates_without_commits_and_stops_for_motion_theme_and_navigation() {
    let app = crate::spec();
    let mut state = crate::evaluation::design();
    crate::home::scene(&mut state, "home");
    let mut rt = Runtime::new(state, app.view, (800.0, 620.0), Env::new(true, 2.0));
    rt.track_marks = true;
    rt.poll = app.poll;
    advance(&mut rt, 1.0);
    let mut draw = Draw::new();
    let before = draw.frame(&mut rt);
    assert!(draw.compositor.animating());
    let commits = rt.stats.commits;
    // Advance the actual render clock; a long jump is intentionally capped.
    let mut travelled = 0;
    for frame in 0..30 {
        advance(&mut rt, 0.1);
        let pixels = draw.frame(&mut rt);
        if frame == 1 {
            travelled = travelled_ink(&before, &pixels, PIXEL_SIZE.0 as usize);
        }
    }
    let after = draw.frame(&mut rt);
    assert_eq!(
        rt.stats.commits, commits,
        "animation must not rebuild the editor"
    );
    let changed = before
        .chunks_exact(4)
        .zip(after.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        changed > 200,
        "the actual shader must animate: {changed} pixels"
    );
    assert!(
        travelled > 100,
        "characters must visibly move within 200 ms, not just change brightness: {travelled}"
    );
    assert!(
        rt.elements().len() < 500,
        "characters must not become UI nodes"
    );

    rt.set_system_reduced_motion(true);
    advance(&mut rt, 1.0);
    let still = draw.frame(&mut rt);
    assert!(!draw.compositor.animating());
    advance(&mut rt, 1.0);
    assert_eq!(
        draw.frame(&mut rt),
        still,
        "Reduce Motion holds a static coloured frame"
    );

    rt.set_system_reduced_motion(false);
    rt.update(|s, _| s.live.as_mut().unwrap().app.preferences.theme = "light".into());
    advance(&mut rt, 1.0);
    draw.frame(&mut rt);
    assert!(!draw.compositor.animating());
    assert!(rt.find("home-ascii").is_none());

    rt.update(|s, _| s.live.as_mut().unwrap().app.preferences.theme = "dark".into());
    advance(&mut rt, 1.0);
    draw.frame(&mut rt);
    assert!(draw.compositor.animating());
    rt.update(|s, _| {
        s.live.as_mut().unwrap().panels.screen = crate::panels::Screen::Conversation;
    });
    advance(&mut rt, 1.0);
    draw.frame(&mut rt);
    assert!(!draw.compositor.animating());
    assert!(rt.find("home-ascii").is_none());
}

fn travelled_ink(before: &[u8], after: &[u8], width: usize) -> usize {
    let height = after.len() / (width * 4);
    (1..height - 1)
        .flat_map(|y| (1..width - 1).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let at = (y * width + x) * 4;
            after[at..at + 3].iter().copied().max().unwrap() > 90
                && (y - 1..=y + 1).all(|row| {
                    (x - 1..=x + 1).all(|col| {
                        let old = (row * width + col) * 4;
                        before[old..old + 3].iter().copied().max().unwrap() < 24
                    })
                })
        })
        .count()
}
