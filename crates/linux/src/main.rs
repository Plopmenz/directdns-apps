//! Linux/desktop runner. All UI code lives in `app-core`.

use app_core::{App, Platform};
use eframe::egui;
use ygg_core::{InterfaceSource, Seed, Session, Status};

struct LinuxPlatform {
    seed: Option<Seed>,
    session: Option<Session>,
}

impl LinuxPlatform {
    fn new() -> Self {
        Self {
            seed: None,
            session: None,
        }
    }
}

impl Platform for LinuxPlatform {
    fn initialize(&mut self) -> Result<String, String> {
        let path = ygg_core::default_seed_path().ok_or("No application data directory found")?;
        let seed = ygg_core::load_or_create_seed(&path)
            .map_err(|error| format!("Cannot load key: {error}"))?;
        let address = ygg_core::address_from_seed(&seed);
        self.seed = Some(seed);
        Ok(address)
    }

    fn status(&self) -> Status {
        self.session
            .as_ref()
            .map(Session::status)
            .unwrap_or_default()
    }

    fn toggle(&mut self) -> Result<(), String> {
        if let Some(mut session) = self.session.take() {
            if let Err(error) = session.stop() {
                self.session = Some(session);
                return Err(format!("Failed to disconnect: {error}"));
            }
            return Ok(());
        }

        let seed = self
            .seed
            .ok_or_else(|| "No private key available".to_string())?;
        let session = Session::start(seed, InterfaceSource::Name("auto".into()))
            .map_err(|error| format!("Failed to connect: {error}"))?;
        self.session = Some(session);
        Ok(())
    }

    fn copy_text(&mut self, ctx: &egui::Context, text: &str) -> Result<(), String> {
        ctx.copy_text(text.to_string());
        Ok(())
    }

    fn take_errors(&mut self) -> Vec<String> {
        Vec::new()
    }
}

struct Runner {
    app: App<LinuxPlatform>,
}

impl eframe::App for Runner {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.logic(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
    }
}

fn main() -> eframe::Result<()> {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([440.0, 420.0])
            .with_min_inner_size([340.0, 340.0]),
        ..Default::default()
    };
    eframe::run_native(
        "DirectDNS",
        options,
        Box::new(|_ctx| {
            Ok(Box::new(Runner {
                app: App::new(LinuxPlatform::new()),
            }) as Box<dyn eframe::App>)
        }),
    )
}
