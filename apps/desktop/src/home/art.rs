//! One retained GPU layer for the character grid; animation never rebuilds Home.
mod gpu;
#[cfg(test)]
mod tests;

use crate::Workspace;
use rune::{El, Frame, compose::DeviceKey, effect, render::KIND_EFFECT};
use std::sync::mpsc::{self, Receiver, TryRecvError};

const MARK: u32 = 0x484f4d45;
const PERIOD: f64 = 8.0;
const WIDTH: usize = 300;
const HEIGHT: usize = 100;
const GLYPHS: &[u8] = b" .:-=+*#%@";
const SOURCE: &str = include_str!("../../assets/home/black-hole.txt");
const COLOURS: &[u8] = include_bytes!("../../assets/home/black-hole.rgb");
const ATLAS: &[u8] = include_bytes!("../../assets/home/glyphs.r8");

pub(super) fn view(reduced_motion: bool) -> El {
    effect(MARK)
        .key("home-ascii")
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .effect_data([f32::from(!reduced_motion), 0.0, 0.0, 0.0])
        .a11y_hidden()
}

enum Preparation {
    Loading(Receiver<gpu::Resources>),
    Ready(Box<gpu::Resources>),
    Unavailable,
}

#[derive(Default)]
enum Motion {
    #[default]
    Rest,
    Orbit {
        phase: f64,
        last: f64,
    },
}

impl Motion {
    fn sample(&mut self, now: f64, animate: bool) -> f32 {
        if !animate {
            *self = Self::Rest;
            return 0.0;
        }
        let phase = match *self {
            Self::Rest => now.rem_euclid(PERIOD),
            Self::Orbit { phase, last } => {
                // Hidden windows do not draw. Resume gently after a long gap.
                (phase + (now - last).clamp(0.0, 0.1)).rem_euclid(PERIOD)
            }
        };
        *self = Self::Orbit { phase, last: now };
        phase as f32
    }
}

#[derive(Default)]
pub(crate) struct Compositor {
    prepared: Option<(DeviceKey, Preparation)>,
    motion: Motion,
}

impl Compositor {
    fn prepare(&mut self, f: &Frame<'_, Workspace>) {
        let key = f.device_key();
        if self.prepared.as_ref().is_none_or(|(old, _)| *old != key) {
            // Only the offline fixture waits. Native windows show the cached PNG
            // while shader compilation and texture upload run off the UI thread.
            let preparation = if f.rt.state.live.as_ref().is_some_and(|l| l.fixture) {
                Preparation::Ready(Box::new(gpu::Resources::new(f.device, f.queue, f.format)))
            } else {
                let (send, receive) = mpsc::sync_channel(1);
                let (device, queue, format) = (f.device.clone(), f.queue.clone(), f.format);
                let handle = f.rt.state.handle.clone();
                std::thread::spawn(move || {
                    let resources = gpu::Resources::new(&device, &queue, format);
                    if send.send(resources).is_ok()
                        && let Some(handle) = handle
                    {
                        handle.wake();
                    }
                });
                Preparation::Loading(receive)
            };
            self.prepared = Some((key, preparation));
        }
        let (_, preparation) = self.prepared.as_mut().unwrap();
        if let Preparation::Loading(receive) = preparation {
            match receive.try_recv() {
                Ok(resources) => *preparation = Preparation::Ready(Box::new(resources)),
                Err(TryRecvError::Disconnected) => *preparation = Preparation::Unavailable,
                Err(TryRecvError::Empty) => {}
            }
        }
    }
}

impl rune::Compositor<Workspace> for Compositor {
    fn animating(&self) -> bool {
        matches!(self.motion, Motion::Orbit { .. })
    }

    fn render(&mut self, f: &mut Frame<'_, Workspace>) {
        let slot =
            f.rt.instances
                .iter()
                .zip(&f.rt.marks)
                .find(|(i, mark)| **mark == MARK && i.params[1] == KIND_EFFECT);
        let Some((slot, _)) = slot else {
            self.motion = Motion::Rest;
            f.draw_ui();
            return;
        };
        let animate = slot.uv[0] > 0.0;
        self.prepare(f);
        let Some((_, Preparation::Ready(resources))) = &self.prepared else {
            self.motion = Motion::Rest;
            f.draw_ui();
            return;
        };
        let phase = self.motion.sample(f.rt.t, animate);
        let clear = f.rt.clear_color();
        f.ui.prepare(
            f.device,
            f.queue,
            &mut f.rt.text.atlas,
            &f.rt.instances,
            f.size,
        );
        let mut shown = false;
        f.ui.draw_with(f.encoder, f.target, clear, &mut |pass, site| {
            if site.mark == MARK {
                shown = true;
                resources.draw(f.queue, pass, site, phase);
            }
        });
        if !shown {
            self.motion = Motion::Rest;
        }
    }
}

/// RGBA8Uint: sampled sRGB ink plus the glyph index. Spaces always remain empty.
fn cells() -> Vec<u8> {
    let mut cells = vec![0; WIDTH * HEIGHT * 4];
    for (y, row) in SOURCE.lines().enumerate().take(HEIGHT) {
        for (x, glyph) in row.bytes().enumerate().take(WIDTH) {
            let Some(index) = GLYPHS.iter().position(|g| *g == glyph).filter(|i| *i > 0) else {
                continue;
            };
            let i = y * WIDTH + x;
            cells[i * 4..i * 4 + 3].copy_from_slice(&COLOURS[i * 3..i * 3 + 3]);
            cells[i * 4 + 3] = index as u8;
        }
    }
    cells
}
