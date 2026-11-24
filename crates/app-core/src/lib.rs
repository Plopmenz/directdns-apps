//! Shared UI state and egui rendering for DirectDNS.
//!
//! Platform crates only provide capabilities Rust cannot do portably: session
//! ownership, clipboard access, Android permissions/VPN service, and error
//! queues. All visible UI, colors, notices, and interaction live here.

use std::time::{Duration, Instant};

use egui::{Color32, RichText};
use ygg_core::Status;

const BACKGROUND_DURATION: f32 = 0.35;
const COPY_NOTICE_DURATION: Duration = Duration::from_secs(2);
const ERROR_NOTICE_DURATION: Duration = Duration::from_secs(6);
const COLOR_OFF: Color32 = Color32::from_rgb(150, 42, 42);
const COLOR_CONNECTING: Color32 = Color32::from_rgb(190, 120, 25);
const COLOR_CONNECTED: Color32 = Color32::from_rgb(30, 130, 80);
const BUTTON_FILL: Color32 = Color32::from_rgb(248, 248, 248);
const BUTTON_TEXT: Color32 = Color32::from_rgb(35, 35, 35);

/// Capabilities the shared UI needs from a platform.
pub trait Platform {
    /// Prepares persistence and returns the raw Yggdrasil address.
    fn initialize(&mut self) -> Result<String, String>;

    /// Current node status.
    fn status(&self) -> Status;

    /// Starts or stops the tunnel. Immediate failures are returned here;
    /// asynchronous Android service failures arrive through [`Self::take_errors`].
    fn toggle(&mut self) -> Result<(), String>;

    /// Copies text using the platform clipboard.
    fn copy_text(&mut self, ctx: &egui::Context, text: &str) -> Result<(), String>;

    /// Drains errors reported asynchronously by platform services.
    fn take_errors(&mut self) -> Vec<String>;
}

struct Notice {
    text: String,
    until: Instant,
}

/// The whole DirectDNS UI, reusable by every native platform.
pub struct App<P: Platform> {
    platform: P,
    address: String,
    background: Color32,
    notice: Option<Notice>,
}

impl<P: Platform> App<P> {
    pub fn new(mut platform: P) -> Self {
        let mut initial_error = None;
        let address = match platform.initialize() {
            Ok(address) => address,
            Err(error) => {
                initial_error = Some(error);
                String::new()
            }
        };

        let mut app = Self {
            platform,
            address,
            background: COLOR_OFF,
            notice: None,
        };
        if let Some(error) = initial_error {
            app.notify(error, ERROR_NOTICE_DURATION);
        }
        app
    }

    fn notify(&mut self, text: impl Into<String>, duration: Duration) {
        self.notice = Some(Notice {
            text: text.into(),
            until: Instant::now() + duration,
        });
    }

    pub fn logic(&mut self, ctx: &egui::Context) {
        for error in self.platform.take_errors() {
            self.notify(error, ERROR_NOTICE_DURATION);
        }
        self.animate_background(ctx);

        if let Some(until) = self.notice.as_ref().map(|notice| notice.until) {
            let now = Instant::now();
            if now >= until {
                self.notice = None;
            } else {
                ctx.request_repaint_after(until - now);
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let running = self.platform.status().running;
        let address = if self.address.is_empty() {
            "ip:unavailable".to_string()
        } else {
            ygg_core::display_address(&self.address)
        };

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.background))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    // `centered_and_justified` does not reliably fill the
                    // CentralPanel in eframe 0.37, so center by explicit space.
                    let content_height = 200.0;
                    ui.add_space((ui.available_height() - content_height).max(0.0) / 2.0);

                    ui.heading(
                        RichText::new("DirectDNS")
                            .size(28.0)
                            .strong()
                            .color(Color32::WHITE),
                    );
                    ui.add_space(32.0);

                    let button = egui::Button::new(
                        RichText::new(if running { "TURN OFF" } else { "TURN ON" })
                            .size(24.0)
                            .strong()
                            .color(BUTTON_TEXT),
                    )
                    .min_size(egui::vec2(240.0, 96.0))
                    .corner_radius(18.0)
                    .fill(BUTTON_FILL);
                    if ui.add(button).clicked() {
                        match self.platform.toggle() {
                            Ok(()) => {}
                            Err(error) => {
                                self.notify(error, ERROR_NOTICE_DURATION);
                            }
                        }
                    }

                    ui.add_space(8.0);
                    if copyable_address(ui, &address) {
                        match self.platform.copy_text(&ctx, &address) {
                            Ok(()) => {
                                self.notify("Copied address to clipboard", COPY_NOTICE_DURATION);
                            }
                            Err(error) => self.notify(error, ERROR_NOTICE_DURATION),
                        }
                    }
                });
            });

        if let Some(notice) = self.notice.as_ref() {
            let notice_id = ui.make_persistent_id("directdns-notice");
            egui::Area::new(notice_id)
                .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0))
                .order(egui::Order::Foreground)
                .interactable(false)
                .fade_in(false)
                .show(&ctx, |ui| {
                    egui::Frame::new()
                        .fill(Color32::from_black_alpha(190))
                        .inner_margin(egui::Margin::symmetric(16, 10))
                        .corner_radius(12.0)
                        .shadow(egui::Shadow {
                            offset: [0, 4],
                            blur: 12,
                            spread: 0,
                            color: Color32::from_black_alpha(70),
                        })
                        .show(ui, |ui| {
                            ui.label(RichText::new(&notice.text).color(Color32::WHITE).size(14.0));
                        });
                });
        }
    }

    fn animate_background(&mut self, ctx: &egui::Context) {
        let status = self.platform.status();
        let target = if !status.running {
            COLOR_OFF
        } else if status.peers_up > 0 {
            COLOR_CONNECTED
        } else {
            COLOR_CONNECTING
        };

        if color_diff(self.background, target) == 0 {
            return;
        }

        let dt = ctx.input(|input| input.stable_dt);
        let step = (dt / BACKGROUND_DURATION).clamp(0.0, 1.0);
        self.background = mix_color(self.background, target, step);
        ctx.request_repaint();
    }
}

/// Draws the address as a subtle clickable chip without adding a visible label.
fn copyable_address(ui: &mut egui::Ui, address: &str) -> bool {
    let galley = ui.fonts_mut(|fonts| {
        fonts.layout_no_wrap(
            address.to_string(),
            egui::FontId::proportional(16.0),
            Color32::WHITE,
        )
    });
    let padding = egui::vec2(10.0, 6.0);
    let (rect, response) =
        ui.allocate_exact_size(galley.size() + padding * 2.0, egui::Sense::click());

    let fill = if response.hovered() {
        Color32::from_white_alpha(30)
    } else {
        Color32::from_white_alpha(12)
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    ui.painter().add(egui::Shape::galley(
        rect.left_top() + padding,
        galley,
        Color32::WHITE,
    ));

    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn mix_color(from: Color32, to: Color32, t: f32) -> Color32 {
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color32::from_rgb(
        mix(from.r(), to.r()),
        mix(from.g(), to.g()),
        mix(from.b(), to.b()),
    )
}

fn color_diff(a: Color32, b: Color32) -> u8 {
    a.r()
        .abs_diff(b.r())
        .max(a.g().abs_diff(b.g()))
        .max(a.b().abs_diff(b.b()))
}
