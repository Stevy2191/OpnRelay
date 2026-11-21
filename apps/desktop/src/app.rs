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

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> App {
        cc.egui_ctx.set_visuals(egui::Visuals::light());
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
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("OpnRelay").strong().size(18.0));
                ui.label(RichText::new("Recorder").color(Color32::GRAY));
            });
            ui.separator();
            match self.screen {
                Screen::Server => {
                    ui.label("Enter your company's OpnRelay address:");
                    ui.text_edit_singleline(&mut self.server_input);
                    if ui.button("Continue").clicked() {
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
                    if ui.button("Sign in with your browser").clicked() {
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
                    ui.label(RichText::new(&self.user).color(Color32::GRAY));
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
                            if ui.button(format!("Retry upload ({} steps)", self.pending.len())).clicked() {
                                self.upload();
                            }
                        } else if ui.button(RichText::new("Start recording").color(Color32::from_rgb(196, 49, 75)).strong()).clicked() {
                            self.start_recording();
                        }
                        ui.label(RichText::new("Clicks on this window are never recorded.").small().color(Color32::GRAY));
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
                            .color(if paused { Color32::GRAY } else { Color32::from_rgb(196, 49, 75) }),
                    );
                    if let Some(e) = err {
                        ui.label(RichText::new(e).small().color(Color32::from_rgb(196, 49, 75)));
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
                        if ui.button(RichText::new("Finish & upload").strong()).clicked()
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
                ui.label(RichText::new(m).color(if *ok { Color32::from_rgb(24, 121, 78) } else { Color32::from_rgb(196, 49, 75) }));
            }
        });
    }
}
