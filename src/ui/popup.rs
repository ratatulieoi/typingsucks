use crate::state::State;
use crossbeam_channel::Receiver;
use eframe::egui;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

const STATE_IDLE: u8 = 0;
const STATE_RECORDING: u8 = 1;
const STATE_TRANSCRIBING: u8 = 2;

pub fn state_to_u8(state: State) -> u8 {
    match state {
        State::Idle | State::Pasting => STATE_IDLE,
        State::Recording => STATE_RECORDING,
        State::Transcribing => STATE_TRANSCRIBING,
    }
}

#[allow(dead_code)]
/// Run the popup UI on the main thread. This blocks.
pub fn run_popup(state: Arc<AtomicU8>, shutdown_rx: Receiver<()>) -> anyhow::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([200.0, 60.0])
            .with_always_on_top()
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false),
        ..Default::default()
    };

    eframe::run_native(
        "typingsucks",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(PopupApp {
                state,
                shutdown_rx,
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("egui error: {}", e))?;

    Ok(())
}

struct PopupApp {
    state: Arc<AtomicU8>,
    shutdown_rx: Receiver<()>,
}

impl eframe::App for PopupApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.shutdown_rx.try_recv().is_ok() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        let current = self.state.load(Ordering::Relaxed);
        let visible = current != STATE_IDLE;

        if visible {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));

            let (text, color) = match current {
                STATE_RECORDING => ("\u{1f3a4} Listening...", egui::Color32::from_rgb(220, 50, 50)),
                STATE_TRANSCRIBING => {
                    ("\u{23f3} Transcribing...", egui::Color32::from_rgb(50, 150, 220))
                }
                _ => ("", egui::Color32::TRANSPARENT),
            };

            egui::CentralPanel::default()
                .frame(
                    egui::Frame::default()
                        .fill(egui::Color32::from_rgba_unmultiplied(30, 30, 30, 230))
                        .rounding(12.0)
                        .inner_margin(16.0),
                )
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new(text).color(color).size(18.0).strong(),
                        );
                    });
                });
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}
