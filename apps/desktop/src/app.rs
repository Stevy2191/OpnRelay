// SPDX-License-Identifier: AGPL-3.0-only
//! The recorder window.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use eframe::egui::{self, Color32, RichText};
use opnrelay_capture::{CaptureMode, Recorder, RecorderOptions, StepDraft};
use opnrelay_client::Workspace;

use crate::session::{self, Shared};
use crate::store::Settings;

enum Msg {
    SignedIn(Result<(Shared, String, Vec<Workspace>), String>),
    Uploaded(Result<String, String>),
    Progress(usize, usize),
}

enum Screen {
    Server,
    SignIn,
    Waiting,
    Ready,
    Recording,
    Uploading,
}

pub struct App {
    settings: Settings,
    screen: Screen,
    server_input: String,
    client: Option<Shared>,
    user: String,
    workspaces: Vec<Workspace>,
    workspace: String,
    title: String,
    mode: CaptureMode,
    recorder: Option<Recorder>,
    pending: Vec<StepDraft>,
    progress: (usize, usize),
    message: Option<(bool, String)>,
    last_url: Option<String>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

fn load_account(client: Shared) -> Result<(Shared, String, Vec<Workspace>), String> {
    let (me, ws) = {
        let mut c = client.lock().expect("lock");
        (
            c.me().map_err(|e| e.to_string())?,
            c.editable_workspaces().map_err(|e| e.to_string())?,
        )
    };
    Ok((
        client,
        format!("{} · {}", me.display_name, me.organization.name),
        ws,
    ))
}

/// Runbook palette tokens for one theme.
struct Palette {
    ink: Color32,
    paper: Color32,
    card: Color32,
    line: Color32,
    text: Color32,
    muted: Color32,
    signal: Color32,
    signal_soft: Color32,
    signal_text: Color32,
    ok: Color32,
    danger: Color32,
}

const LIGHT: Palette = Palette {
    ink: Color32::from_rgb(0x0f, 0x1a, 0x2b),
    paper: Color32::from_rgb(0xf5, 0xf6, 0xf8),
    card: Color32::from_rgb(0xff, 0xff, 0xff),
    line: Color32::from_rgb(0xdf, 0xe3, 0xea),
    text: Color32::from_rgb(0x18, 0x22, 0x35),
    muted: Color32::from_rgb(0x62, 0x70, 0x89),
    signal: Color32::from_rgb(0xff, 0x6b, 0x1a),
    signal_soft: Color32::from_rgb(0xff, 0xf0, 0xe6),
    signal_text: Color32::from_rgb(0xb1, 0x47, 0x08),
    ok: Color32::from_rgb(0x13, 0x8a, 0x5e),
    danger: Color32::from_rgb(0xb4, 0x23, 0x18),
};

const DARK: Palette = Palette {
    ink: Color32::from_rgb(0x0b, 0x14, 0x24),
    paper: Color32::from_rgb(0x0e, 0x16, 0x26),
    card: Color32::from_rgb(0x15, 0x20, 0x36),
    line: Color32::from_rgb(0x26, 0x34, 0x4d),
    text: Color32::from_rgb(0xe6, 0xeb, 0xf3),
    muted: Color32::from_rgb(0x93, 0xa1, 0xb8),
    signal: Color32::from_rgb(0xff, 0x7a, 0x2e),
    signal_soft: Color32::from_rgb(0x3a, 0x24, 0x14),
    signal_text: Color32::from_rgb(0xff, 0xb2, 0x7f),
    ok: Color32::from_rgb(0x3c, 0xcf, 0x91),
    danger: Color32::from_rgb(0xff, 0x7b, 0x72),
};

fn palette(ui: &egui::Ui) -> &'static Palette {
    if ui.visuals().dark_mode {
        &DARK
    } else {
        &LIGHT
    }
}

fn runbook_visuals(base: egui::Visuals, p: &Palette) -> egui::Visuals {
    let mut v = base;
    v.override_text_color = Some(p.text);
    v.panel_fill = p.paper;
    v.window_fill = p.card;
    v.extreme_bg_color = p.card;
    v.faint_bg_color = p.card;
    v.hyperlink_color = p.signal_text;
    v.error_fg_color = p.danger;
    v.selection.bg_fill = p.signal_soft;
    v.selection.stroke = egui::Stroke::new(1.0, p.signal_text);
    v.widgets.noninteractive.bg_fill = p.card;
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, p.line);
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, p.text);
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.bg_fill = p.card;
        w.weak_bg_fill = p.card;
        w.bg_stroke = egui::Stroke::new(1.0, p.line);
        w.fg_stroke = egui::Stroke::new(1.0, p.text);
        w.corner_radius = 6.0.into();
    }
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, p.signal);
    v.widgets.active.bg_stroke = egui::Stroke::new(1.0, p.signal);
    v
}

/// Apply the Runbook palette to both egui themes; egui follows the OS light/dark setting.
fn apply_runbook_theme(ctx: &egui::Context) {
    ctx.set_visuals_of(
        egui::Theme::Light,
        runbook_visuals(egui::Visuals::light(), &LIGHT),
    );
    ctx.set_visuals_of(
        egui::Theme::Dark,
        runbook_visuals(egui::Visuals::dark(), &DARK),
    );
}

/// The orange primary button (navy label on signal orange).
fn primary_button(ui: &mut egui::Ui, label: impl Into<String>) -> egui::Response {
    let p = palette(ui);
    ui.add(
        egui::Button::new(RichText::new(label).strong().color(LIGHT.ink))
            .fill(p.signal)
            .stroke(egui::Stroke::new(1.0, p.signal)),
    )
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> App {
        apply_runbook_theme(&cc.egui_ctx);
        let settings = Settings::load();
        let (tx, rx) = channel();
        let mode = CaptureMode::parse(settings.capture_mode.as_deref().unwrap_or("region"));
        let app = App {
            server_input: settings.server_url.clone().unwrap_or_default(),
            screen: if settings.server_url.is_some() {
                Screen::Waiting
            } else {
                Screen::Server
            },
            settings,
            client: None,
            user: String::new(),
            workspaces: vec![],
            workspace: String::new(),
            title: String::new(),
            mode,
            recorder: None,
            pending: vec![],
            progress: (0, 0),
            message: None,
            last_url: None,
            tx,
            rx,
        };
        if let Some(server) = app.settings.server_url.clone() {
            let tx = app.tx.clone();
            std::thread::spawn(move || {
                let r = match session::resume(&server) {
                    Some(c) => load_account(c),
                    None => Err(String::new()),
                };
                let _ = tx.send(Msg::SignedIn(r));
            });
        }
        app
    }

    fn server(&self) -> String {
        self.settings.server_url.clone().unwrap_or_default()
    }

    fn handle_messages(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::SignedIn(Ok((client, user, ws))) => {
                    self.client = Some(client);
                    self.user = user;
                    let last = self.settings.last_workspace.clone().unwrap_or_default();
                    self.workspace = ws
                        .iter()
                        .find(|w| w.id == last)
                        .or(ws.first())
                        .map(|w| w.id.clone())
                        .unwrap_or_default();
                    self.workspaces = ws;
                    self.screen = Screen::Ready;
                }
                Msg::SignedIn(Err(e)) => {
                    self.screen = Screen::SignIn;
                    if !e.is_empty() {
                        self.message = Some((false, e));
                    }
                }
                Msg::Progress(a, b) => self.progress = (a, b),
                Msg::Uploaded(Ok(url)) => {
                    self.pending.clear();
                    let _ = webbrowser::open(&url);
                    self.last_url = Some(url);
                    self.message = Some((
                        true,
                        "Uploaded. The guide opened in your browser for editing.".into(),
                    ));
                    self.title.clear();
                    self.screen = Screen::Ready;
                }
                Msg::Uploaded(Err(e)) => {
                    // Keep the steps so the upload can be retried.
                    self.message = Some((
                        false,
                        format!("Upload failed: {e}. Your steps are kept; try again."),
                    ));
                    self.screen = Screen::Ready;
                }
            }
        }
    }

    fn start_sign_in(&mut self) {
        let server = self.server();
        match session::begin_sign_in(&server) {
            Ok((url, wait)) => {
                let _ = webbrowser::open(&url);
                self.screen = Screen::Waiting;
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let r = wait().map_err(|e| e.to_string()).and_then(load_account);
                    let _ = tx.send(Msg::SignedIn(r));
                });
            }
            Err(e) => self.message = Some((false, e.to_string())),
        }
    }

    fn start_recording(&mut self) {
        match Recorder::start(RecorderOptions {
            mode: self.mode,
            ..Default::default()
        }) {
            Ok(r) => {
                self.recorder = Some(r);
                self.message = None;
                self.screen = Screen::Recording;
                self.settings.last_workspace = Some(self.workspace.clone());
                self.settings.capture_mode = Some(
                    match self.mode {
                        CaptureMode::Full => "full",
                        CaptureMode::Window => "window",
                        CaptureMode::Region => "region",
                    }
                    .into(),
                );
                self.settings.save();
            }
            Err(e) => self.message = Some((false, e)),
        }
    }

    fn upload(&mut self) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let steps = std::mem::take(&mut self.pending);
        self.pending = steps.clone();
        let (ws, title) = (
            self.workspace.clone(),
            if self.title.trim().is_empty() {
                "New guide".to_string()
            } else {
                self.title.trim().to_string()
            },
        );
        let tx = self.tx.clone();
        self.screen = Screen::Uploading;
        self.progress = (0, steps.len());
        std::thread::spawn(move || {
            let ptx = tx.clone();
            let r = session::upload(&client, &ws, &title, &steps, |a, b| {
                let _ = ptx.send(Msg::Progress(a, b));
            });
            let _ = tx.send(Msg::Uploaded(r.map_err(|e| e.to_string())));
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_messages();
        ui.ctx().request_repaint_after(Duration::from_millis(250));
        let p = palette(ui);
        egui::Panel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(p.ink)
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("OpnRelay")
                            .strong()
                            .size(18.0)
                            .color(Color32::WHITE),
                    );
                    ui.label(RichText::new("Recorder").color(Color32::from_rgb(0xb8, 0xc2, 0xd3)));
                });
            });
        egui::CentralPanel::default().show(ui, |ui| {
            match self.screen {
                Screen::Server => {
                    ui.label("Enter your company's OpnRelay address:");
                    ui.text_edit_singleline(&mut self.server_input);
                    if primary_button(ui, "Continue").clicked() {
                        match opnrelay_client::normalize_server(&self.server_input) {
                            Some(url) => {
                                self.settings.server_url = Some(url);
                                self.settings.save();
                                self.message = None;
                                self.screen = Screen::SignIn;
                            }
                            None => self.message = Some((false, "Enter an https:// address, e.g. https://guides.company.com".into())),
                        }
                    }
                }
                Screen::SignIn => {
                    ui.label(format!("Server: {}", self.server()));
                    if primary_button(ui, "Sign in with your browser").clicked() {
                        self.start_sign_in();
                    }
                    if ui.small_button("Use a different server").clicked() {
                        self.screen = Screen::Server;
                    }
                }
                Screen::Waiting => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Waiting for sign-in in your browser…");
                    });
                }
                Screen::Ready => {
                    ui.label(RichText::new(&self.user).color(p.muted));
                    if self.workspaces.is_empty() {
                        ui.label("You can't create guides yet. Ask an administrator to make you an editor in a workspace.");
                    } else {
                        ui.label("Guide title");
                        ui.add(egui::TextEdit::singleline(&mut self.title).hint_text("e.g. Run the monthly payroll"));
                        ui.label("Workspace");
                        let current = self.workspaces.iter().find(|w| w.id == self.workspace).map(|w| w.name.clone()).unwrap_or_default();
                        egui::ComboBox::from_id_salt("ws").selected_text(current).show_ui(ui, |ui| {
                            for w in &self.workspaces {
                                ui.selectable_value(&mut self.workspace, w.id.clone(), &w.name);
                            }
                        });
                        ui.label("Screenshots");
                        ui.radio_value(&mut self.mode, CaptureMode::Region, "Area around each click");
                        ui.radio_value(&mut self.mode, CaptureMode::Window, "Active window");
                        ui.radio_value(&mut self.mode, CaptureMode::Full, "Full screen");
                        ui.add_space(6.0);
                        if !self.pending.is_empty() {
                            if primary_button(ui, format!("Retry upload ({} steps)", self.pending.len())).clicked() {
                                self.upload();
                            }
                        } else if primary_button(ui, "● Start recording").clicked() {
                            self.start_recording();
                        }
                        ui.label(RichText::new("Clicks on this window are never recorded.").small().color(p.muted));
                    }
                    if let Some(url) = self.last_url.clone()
                        && ui.link("Open the last guide").clicked()
                    {
                        let _ = webbrowser::open(&url);
                    }
                    ui.add_space(8.0);
                    if ui.small_button("Sign out").clicked() {
                        if let Some(c) = self.client.take() {
                            c.lock().expect("lock").revoke();
                        }
                        crate::store::clear_refresh(&self.server());
                        self.screen = Screen::SignIn;
                    }
                }
                Screen::Recording => {
                    let (count, paused, err) = self
                        .recorder
                        .as_ref()
                        .map(|r| (r.step_count(), r.is_paused(), r.last_error()))
                        .unwrap_or_default();
                    ui.label(
                        RichText::new(format!("{} · {count} steps", if paused { "Paused" } else { "Recording" }))
                            .size(16.0)
                            .monospace()
                            .color(if paused { p.muted } else { p.danger }),
                    );
                    if let Some(e) = err {
                        ui.label(RichText::new(e).small().color(p.danger));
                    }
                    ui.horizontal(|ui| {
                        if ui.button(if paused { "Resume" } else { "Pause" }).clicked()
                            && let Some(r) = &self.recorder
                        {
                            r.set_paused(!paused);
                        }
                        if ui.button("Undo last step").clicked()
                            && let Some(r) = &self.recorder
                        {
                            r.undo_last();
                        }
                    });
                    ui.horizontal(|ui| {
                        if primary_button(ui, "Finish & upload").clicked()
                            && let Some(r) = self.recorder.take()
                        {
                            self.pending = r.finish();
                            if self.pending.is_empty() {
                                self.message = Some((false, "No steps were recorded.".into()));
                                self.screen = Screen::Ready;
                            } else {
                                self.upload();
                            }
                        }
                        if ui.button("Discard").clicked() {
                            self.recorder = None;
                            self.screen = Screen::Ready;
                        }
                    });
                }
                Screen::Uploading => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(format!("Uploading {} of {}…", self.progress.0.min(self.progress.1), self.progress.1));
                    });
                }
            }
            if let Some((ok, m)) = &self.message {
                ui.add_space(6.0);
                ui.label(RichText::new(m).color(if *ok { p.ok } else { p.danger }));
            }
        });
    }
}
