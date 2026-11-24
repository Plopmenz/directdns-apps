//! Native egui UI for Linux/desktop.

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};
use ygg_core::{InterfaceSource, Seed, Session, Status, display_address};

struct App {
    seed: Option<Seed>,
    address: String,
    session: Option<Session>,
    status: Status,
    error: Option<String>,
    copied_until: Option<Instant>,
}

impl App {
    fn new() -> Self {
        let mut error = None;
        let mut seed = None;
        let mut address = String::new();

        match ygg_core::default_seed_path() {
            Some(path) => match ygg_core::load_or_create_seed(&path) {
                Ok(loaded) => {
                    address = ygg_core::address_from_seed(&loaded);
                    seed = Some(loaded);
                }
                Err(err) => error = Some(format!("Cannot load key: {err}")),
            },
            None => error = Some("No application data directory found".into()),
        }

        Self {
            seed,
            address,
            session: None,
            status: Status::default(),
            error,
            copied_until: None,
        }
    }

    fn toggle(&mut self) {
        self.error = None;
        if let Some(mut session) = self.session.take() {
            if let Err(err) = session.stop() {
                self.error = Some(format!("Failed to disconnect: {err}"));
            }
            self.status = Status::default();
            return;
        }

        let Some(seed) = self.seed else {
            self.error = Some("No private key available".into());
            return;
        };

        match Session::start(seed, InterfaceSource::Name("auto".into())) {
            Ok(session) => {
                self.status = session.status();
                self.session = Some(session);
            }
            Err(err) => self.error = Some(err.0),
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(session) = &self.session {
            self.status = session.status();
        }
        if let Some(until) = self.copied_until
            && until > Instant::now()
        {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(16, 8))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(18.0);
                    ui.heading(RichText::new("DirectDNS").size(28.0).strong());
                    ui.add_space(6.0);
                    ui.label("Private Yggdrasil peer with global DNS");
                    ui.add_space(22.0);

                    let running = self.status.running;
                    let button = egui::Button::new(
                        RichText::new(if running { "Turn off" } else { "Turn on" })
                            .size(26.0)
                            .strong(),
                    )
                    .min_size(egui::vec2(230.0, 96.0))
                    .corner_radius(18.0)
                    .fill(if running {
                        Color32::from_rgb(35, 120, 86)
                    } else {
                        Color32::from_rgb(52, 100, 180)
                    });
                    if ui.add(button).clicked() {
                        self.toggle();
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new(&self.status.message).color(
                        if self.status.running && self.status.peers_up > 0 {
                            Color32::from_rgb(60, 180, 120)
                        } else {
                            Color32::GRAY
                        },
                    ));

                    ui.add_space(24.0);
                    ui.separator();
                    ui.add_space(12.0);
                    ui.label(RichText::new("Your Yggdrasil address").strong());
                    ui.add_space(4.0);

                    if self.address.is_empty() {
                        ui.label("unavailable");
                    } else {
                        let text = display_address(&self.address);
                        let chip = egui::Button::new(RichText::new(&text).monospace().size(15.0))
                            .frame(false)
                            .corner_radius(6.0);
                        if ui.add(chip).clicked() {
                            ctx.copy_text(text);
                            self.copied_until = Some(Instant::now() + Duration::from_secs(2));
                        }
                        if self
                            .copied_until
                            .is_some_and(|until| until > Instant::now())
                        {
                            ui.label(RichText::new("Copied!").small().color(Color32::GREEN));
                        } else {
                            ui.label(RichText::new("Click to copy").small().weak());
                        }
                    }

                    ui.add_space(18.0);
                    ui.separator();
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(format!(
                            "Peers: {}/{} up",
                            self.status.peers_up, self.status.peers_total
                        ))
                        .small(),
                    );
                    ui.label(RichText::new("Routing: 200::/7 via Yggdrasil").small());
                    if self.status.dns_servers.is_empty() {
                        ui.label(RichText::new("DNS: system default").small());
                    } else {
                        let servers = self
                            .status
                            .dns_servers
                            .iter()
                            .map(|ip| format!("{ip}:53"))
                            .collect::<Vec<_>>()
                            .join(", ");
                        ui.label(RichText::new(format!("DNS: {servers}")).small());
                    }
                });

                if let Some(error) = &self.error {
                    ui.add_space(12.0);
                    egui::Frame::new()
                        .fill(Color32::from_rgb(90, 35, 35))
                        .corner_radius(8.0)
                        .inner_margin(10.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new(error).color(Color32::from_rgb(255, 190, 190)));
                        });
                }
            });
    }
}

fn main() -> eframe::Result<()> {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([440.0, 620.0])
            .with_min_inner_size([340.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "DirectDNS",
        options,
        Box::new(|_ctx| Ok(Box::new(App::new()) as Box<dyn eframe::App>)),
    )
}
