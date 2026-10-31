//! Interfaccia grafica (egui).
use crate::config::{Config, Mode};
use crate::downloader::{self, Emit, Event, Toolset};
use crate::selfupdate::{self, Release};
use crate::tools;
use eframe::egui::{self, Color32, RichText};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;

const ACCENT: Color32 = Color32::from_rgb(142, 84, 233);
const OK: Color32 = Color32::from_rgb(64, 185, 120);
const ERR: Color32 = Color32::from_rgb(235, 87, 87);
const WARN: Color32 = Color32::from_rgb(230, 160, 50);

const VIDEO_FORMATS: [(&str, &str); 3] = [("mp4", "MP4"), ("mkv", "MKV"), ("webm", "WebM")];
const HEIGHTS: [(u32, &str); 7] = [
    (0, "Migliore disponibile"),
    (2160, "4K (2160p)"),
    (1440, "1440p"),
    (1080, "Full HD (1080p)"),
    (720, "HD (720p)"),
    (480, "480p"),
    (360, "360p"),
];
const AUDIO_FORMATS: [(&str, &str); 6] = [
    ("mp3", "MP3"),
    ("m4a", "M4A (AAC)"),
    ("opus", "Opus"),
    ("vorbis", "OGG Vorbis"),
    ("flac", "FLAC (senza perdita)"),
    ("wav", "WAV (senza perdita)"),
];
const BITRATES: [u32; 7] = [320, 256, 192, 160, 128, 96, 64];
const BROWSERS: [(&str, &str); 7] = [
    ("", "Nessuno"),
    ("chrome", "Chrome"),
    ("firefox", "Firefox"),
    ("edge", "Edge"),
    ("brave", "Brave"),
    ("opera", "Opera"),
    ("safari", "Safari"),
];

enum Msg {
    ToolsStatus(String),
    ToolsReady {
        ytdlp: String,
        ffmpeg: Option<PathBuf>,
        deno: Option<PathBuf>,
        warnings: Vec<String>,
    },
    ToolsFailed(String),
    Release(Option<Release>),
    UpdateStatus(String),
    UpdateDone(Result<(), String>),
    Folder(PathBuf),
    Dl(Event),
}

#[derive(Clone, Copy, PartialEq)]
enum JobState {
    Waiting,
    Running,
    Done,
    Failed,
}

struct JobUi {
    id: usize,
    label: String,
    pct: f32,
    detail: String,
    state: JobState,
}

enum UpdateUi {
    None,
    Available(Release),
    Working(String),
    Ready(String),
    Failed(String),
}

#[derive(Default)]
struct ToolsUi {
    ytdlp: Option<String>,
    ffmpeg: Option<PathBuf>,
    deno: Option<PathBuf>,
    status: String,
    error: Option<String>,
    warnings: Vec<String>,
    busy: bool,
}

pub struct App {
    cfg: Config,
    input: String,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    tools: ToolsUi,
    jobs: Vec<JobUi>,
    next_id: usize,
    running: bool,
    cancel: Arc<AtomicBool>,
    log: Vec<String>,
    update: UpdateUi,
    logo: egui::TextureHandle,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_style(&cc.egui_ctx);
        let logo = load_logo(&cc.egui_ctx);
        let (tx, rx) = channel();
        let mut app = Self {
            cfg: Config::load(),
            input: String::new(),
            tx,
            rx,
            tools: ToolsUi::default(),
            jobs: Vec::new(),
            next_id: 0,
            running: false,
            cancel: Arc::new(AtomicBool::new(false)),
            log: Vec::new(),
            update: UpdateUi::None,
            logo,
        };
        app.start_setup(&cc.egui_ctx);
        app
    }

    fn sender(&self, ctx: &egui::Context) -> impl Fn(Msg) + Send + Clone + 'static {
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        move |m| {
            let _ = tx.send(m);
            ctx.request_repaint();
        }
    }

    /// Scarica/aggiorna yt-dlp, Deno e ffmpeg, poi controlla gli aggiornamenti dell'app.
    fn start_setup(&mut self, ctx: &egui::Context) {
        self.tools.busy = true;
        self.tools.error = None;
        self.tools.status = "Preparazione…".into();
        let send = self.sender(ctx);
        thread::spawn(move || {
            let mut status = |s: String| send(Msg::ToolsStatus(s));
            let ytdlp = match tools::ensure_ytdlp(&mut status) {
                Ok(v) => v,
                Err(e) => {
                    send(Msg::ToolsFailed(format!("{e:#}")));
                    return;
                }
            };
            let mut warnings = Vec::new();
            let deno = tools::ensure_deno(&mut status)
                .map_err(|e| warnings.push(format!("Deno non disponibile ({e:#}): alcuni video YouTube potrebbero non funzionare.")))
                .ok();
            let ffmpeg = tools::ensure_ffmpeg(&mut status)
                .map_err(|e| warnings.push(format!("ffmpeg non disponibile ({e:#}): servono per conversione audio e unione video/audio. Installalo e riavvia.")))
                .ok();
            send(Msg::ToolsReady {
                ytdlp,
                ffmpeg,
                deno,
                warnings,
            });
            send(Msg::Release(selfupdate::check().unwrap_or(None)));
        });
    }

    fn start_update(&mut self, ctx: &egui::Context, rel: Release) {
        self.update = UpdateUi::Working("Avvio aggiornamento…".into());
        let send = self.sender(ctx);
        thread::spawn(move || {
            let mut st = |s: String| send(Msg::UpdateStatus(s));
            let r = selfupdate::apply(&rel, &mut st).map_err(|e| format!("{e:#}"));
            send(Msg::UpdateDone(r));
        });
    }

    fn start_download(&mut self, ctx: &egui::Context) {
        let inputs: Vec<String> = self
            .input
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if inputs.is_empty() {
            return;
        }
        if let Err(e) = std::fs::create_dir_all(&self.cfg.out_dir) {
            self.log
                .push(format!("Impossibile creare la cartella: {e}"));
            return;
        }
        self.cfg.save();
        self.jobs.clear();
        self.running = true;
        self.cancel.store(false, Ordering::Relaxed);

        let toolset = Toolset {
            ytdlp: tools::ytdlp_path(),
            ffmpeg_dir: self.tools.ffmpeg.clone(),
            deno: self.tools.deno.clone(),
        };
        let send = self.sender(ctx);
        let emit: Emit = Arc::new(move |e| send(Msg::Dl(e)));
        let (cfg, cancel, base) = (self.cfg.clone(), self.cancel.clone(), self.next_id);
        thread::spawn(move || downloader::run(inputs, cfg, toolset, base, emit, cancel));
    }

    fn pick_folder(&self, ctx: &egui::Context) {
        let send = self.sender(ctx);
        let start = self.cfg.out_dir.clone();
        thread::spawn(move || {
            if let Some(p) = rfd::FileDialog::new()
                .set_title("Scegli la cartella di download")
                .set_directory(start)
                .pick_folder()
            {
                send(Msg::Folder(p));
            }
        });
    }

    fn handle(&mut self, msg: Msg, ctx: &egui::Context) {
        match msg {
            Msg::ToolsStatus(s) => self.tools.status = s,
            Msg::ToolsReady {
                ytdlp,
                ffmpeg,
                deno,
                warnings,
            } => {
                self.tools.busy = false;
                self.tools.ytdlp = Some(ytdlp);
                self.tools.ffmpeg = ffmpeg;
                self.tools.deno = deno;
                self.tools.warnings = warnings;
            }
            Msg::ToolsFailed(e) => {
                self.tools.busy = false;
                self.tools.error = Some(e);
            }
            Msg::Release(Some(rel)) => {
                if self.cfg.auto_update_app && selfupdate::supported() && rel.asset_url.is_some() {
                    self.start_update(ctx, rel);
                } else {
                    self.update = UpdateUi::Available(rel);
                }
            }
            Msg::Release(None) => {}
            Msg::UpdateStatus(s) => self.update = UpdateUi::Working(s),
            Msg::UpdateDone(Ok(())) => {
                self.update =
                    UpdateUi::Ready("Aggiornamento installato. Riavvia per usarlo.".into())
            }
            Msg::UpdateDone(Err(e)) => self.update = UpdateUi::Failed(e),
            Msg::Folder(p) => self.cfg.out_dir = p,
            Msg::Dl(ev) => match ev {
                Event::Log(l) => {
                    self.log.push(l);
                    if self.log.len() > 2000 {
                        self.log.drain(..500);
                    }
                }
                Event::JobAdded { id, label } => {
                    self.next_id = self.next_id.max(id + 1);
                    self.jobs.push(JobUi {
                        id,
                        label,
                        pct: 0.0,
                        detail: String::new(),
                        state: JobState::Waiting,
                    });
                }
                Event::JobLabel { id, label } => {
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        j.label = label;
                    }
                }
                Event::JobProgress { id, pct, detail } => {
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        j.state = JobState::Running;
                        j.pct = pct;
                        j.detail = detail;
                    }
                }
                Event::JobFinished { id, ok, detail } => {
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        j.state = if ok { JobState::Done } else { JobState::Failed };
                        j.pct = if ok { 1.0 } else { j.pct };
                        j.detail = detail;
                    }
                }
                Event::Finished => self.running = false,
            },
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(m) = self.rx.try_recv() {
            self.handle(m, ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.set_max_width(ui.available_width());
                    self.header(ui);
                    ui.add_space(8.0);
                    self.update_banner(ui, ctx);
                    self.tools_banner(ui, ctx);
                    ui.add_space(4.0);
                    self.link_card(ui);
                    ui.add_space(8.0);
                    self.options_card(ui, ctx);
                    ui.add_space(10.0);
                    self.action_row(ui, ctx);
                    ui.add_space(10.0);
                    self.queue(ui);
                    self.log_view(ui);
                    ui.add_space(6.0);
                    self.footer(ui);
                });
        });
    }
}

// ───────────────────────────── sezioni UI ─────────────────────────────

fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::group(ui.style())
        .corner_radius(10)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

impl App {
    fn header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.image((self.logo.id(), egui::vec2(46.0, 46.0)));
            ui.vertical(|ui| {
                ui.label(RichText::new("Grabbit").size(24.0).strong());
                ui.label(
                    RichText::new("Video e musica da YouTube e Spotify")
                        .color(ui.visuals().weak_text_color()),
                );
            });
        });
    }

    fn update_banner(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut start: Option<Release> = None;
        let mut restart = false;
        match &self.update {
            UpdateUi::None => return,
            UpdateUi::Available(rel) => {
                card(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("Nuova versione {} disponibile", rel.version))
                                .strong()
                                .color(ACCENT),
                        );
                        if selfupdate::supported() && rel.asset_url.is_some() {
                            if ui.button("Aggiorna ora").clicked() {
                                start = Some(rel.clone());
                            }
                        } else if !rel.page_url.is_empty() {
                            ui.hyperlink_to("Scarica dal sito", &rel.page_url);
                        }
                    });
                    let notes = rel.notes.trim();
                    if !notes.is_empty() {
                        ui.collapsing("Novità", |ui| {
                            ui.label(RichText::new(notes).small());
                        });
                    }
                });
            }
            UpdateUi::Working(s) => {
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(s);
                    });
                });
            }
            UpdateUi::Ready(s) => {
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(s).color(OK));
                        if ui.button("Riavvia ora").clicked() {
                            restart = true;
                        }
                    });
                });
            }
            UpdateUi::Failed(e) => {
                card(ui, |ui| {
                    ui.label(RichText::new(format!("Aggiornamento non riuscito: {e}")).color(ERR));
                });
            }
        }
        if let Some(r) = start {
            self.start_update(ctx, r);
        }
        if restart {
            selfupdate::restart();
        }
        ui.add_space(6.0);
    }

    fn tools_banner(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.tools.busy {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(&self.tools.status);
                });
            });
            ui.add_space(6.0);
        }
        if let Some(e) = self.tools.error.clone() {
            card(ui, |ui| {
                ui.label(
                    RichText::new("Impossibile preparare yt-dlp")
                        .strong()
                        .color(ERR),
                );
                ui.label(e);
                if ui.button("Riprova").clicked() {
                    self.start_setup(ctx);
                }
            });
            ui.add_space(6.0);
        }
        for w in &self.tools.warnings {
            ui.label(RichText::new(format!("Attenzione: {w}")).color(WARN));
        }
    }

    fn link_card(&mut self, ui: &mut egui::Ui) {
        let field_id = ui.scope_id().with("link_field");
        card(ui, |ui| {
            let mut paste = false;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Link").strong());
                paste = ui.small_button("Incolla").clicked();
                if ui.small_button("Svuota").clicked() {
                    self.input.clear();
                }
            });
            let resp = ui.add(
                egui::TextEdit::multiline(&mut self.input)
                    .id(field_id)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .hint_text(
                        "Incolla qui uno o più link YouTube o Spotify, uno per riga (Ctrl+V)",
                    ),
            );
            resp.context_menu(|ui| {
                if ui.button("Incolla").clicked() {
                    paste = true;
                    ui.close();
                }
                if ui.button("Svuota").clicked() {
                    self.input.clear();
                    ui.close();
                }
            });
            if paste {
                // il testo incollato va al campo che ha il focus: glielo ridiamo
                ui.memory_mut(|m| m.request_focus(field_id));
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            }
        });
    }

    fn options_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.cfg.mode,
                    Mode::Video,
                    RichText::new("  Video  ").size(16.0),
                );
                ui.selectable_value(
                    &mut self.cfg.mode,
                    Mode::Audio,
                    RichText::new("  Audio (musica)  ").size(16.0),
                );
            });
            ui.add_space(6.0);

            egui::Grid::new("opts")
                .num_columns(2)
                .spacing([14.0, 8.0])
                .show(ui, |ui| {
                    match self.cfg.mode {
                        Mode::Video => {
                            ui.label("Formato");
                            egui::ComboBox::from_id_salt("vf")
                                .selected_text(label_of(&VIDEO_FORMATS, &self.cfg.video_container))
                                .show_ui(ui, |ui| {
                                    for (v, l) in VIDEO_FORMATS {
                                        ui.selectable_value(
                                            &mut self.cfg.video_container,
                                            v.to_string(),
                                            l,
                                        );
                                    }
                                });
                            ui.end_row();
                            ui.label("Qualità massima");
                            egui::ComboBox::from_id_salt("vh")
                                .selected_text(
                                    HEIGHTS
                                        .iter()
                                        .find(|h| h.0 == self.cfg.max_height)
                                        .map(|h| h.1)
                                        .unwrap_or("Migliore disponibile"),
                                )
                                .show_ui(ui, |ui| {
                                    for (v, l) in HEIGHTS {
                                        ui.selectable_value(&mut self.cfg.max_height, v, l);
                                    }
                                });
                            ui.end_row();
                        }
                        Mode::Audio => {
                            ui.label("Formato");
                            egui::ComboBox::from_id_salt("af")
                                .selected_text(label_of(&AUDIO_FORMATS, &self.cfg.audio_format))
                                .show_ui(ui, |ui| {
                                    for (v, l) in AUDIO_FORMATS {
                                        ui.selectable_value(
                                            &mut self.cfg.audio_format,
                                            v.to_string(),
                                            l,
                                        );
                                    }
                                });
                            ui.end_row();
                            ui.label("Bitrate");
                            let lossless = matches!(self.cfg.audio_format.as_str(), "flac" | "wav");
                            ui.add_enabled_ui(!lossless, |ui| {
                                egui::ComboBox::from_id_salt("ab")
                                    .selected_text(if lossless {
                                        "—".to_string()
                                    } else {
                                        format!("{} kbps", self.cfg.audio_bitrate)
                                    })
                                    .show_ui(ui, |ui| {
                                        for b in BITRATES {
                                            ui.selectable_value(
                                                &mut self.cfg.audio_bitrate,
                                                b,
                                                format!("{b} kbps"),
                                            );
                                        }
                                    });
                            });
                            ui.end_row();
                        }
                    }
                    ui.label("Salva in");
                    ui.horizontal(|ui| {
                        if ui.button("Sfoglia…").clicked() {
                            self.pick_folder(ctx);
                        }
                        ui.label(self.cfg.out_dir.to_string_lossy());
                    });
                    ui.end_row();
                });

            ui.add_space(4.0);
            ui.checkbox(
                &mut self.cfg.playlist,
                "Scarica l'intera playlist (se il link è una playlist)",
            );
            ui.checkbox(&mut self.cfg.embed_meta, "Incorpora copertina e metadati");
            if self.cfg.mode == Mode::Video {
                ui.label(
                    RichText::new("I link Spotify vengono sempre scaricati come audio.")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            }

            ui.collapsing("Opzioni avanzate", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Usa i cookie del browser");
                    egui::ComboBox::from_id_salt("ck")
                        .selected_text(label_of(&BROWSERS, &self.cfg.cookies_browser))
                        .show_ui(ui, |ui| {
                            for (v, l) in BROWSERS {
                                ui.selectable_value(&mut self.cfg.cookies_browser, v.to_string(), l);
                            }
                        });
                });
                ui.label(RichText::new("Utile per video con restrizioni d'età o quando YouTube chiede di dimostrare che non sei un robot.").small().color(ui.visuals().weak_text_color()));
                ui.checkbox(&mut self.cfg.auto_update_app, "Installa automaticamente gli aggiornamenti di Grabbit");
                ui.horizontal(|ui| {
                    if ui.add_enabled(!self.tools.busy && !self.running, egui::Button::new("Aggiorna yt-dlp ora")).clicked() {
                        self.start_setup(ctx);
                    }
                    if ui.button("Apri cartella dei download").clicked() {
                        open_folder(&self.cfg.out_dir);
                    }
                });
            });
        });
    }

    fn action_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let has_input = self.input.lines().any(|l| !l.trim().is_empty());
        let ready = self.tools.ytdlp.is_some() && !self.tools.busy;
        ui.horizontal(|ui| {
            if self.running {
                let btn = egui::Button::new(RichText::new("   Annulla   ").size(17.0))
                    .min_size(egui::vec2(0.0, 38.0));
                if ui.add(btn).clicked() {
                    self.cancel.store(true, Ordering::Relaxed);
                }
                ui.spinner();
            } else {
                let btn = egui::Button::new(
                    RichText::new("   Scarica   ")
                        .size(17.0)
                        .strong()
                        .color(Color32::WHITE),
                )
                .fill(ACCENT)
                .min_size(egui::vec2(0.0, 38.0));
                if ui.add_enabled(ready && has_input, btn).clicked() {
                    self.start_download(ctx);
                }
                if !self.jobs.is_empty() && ui.button("Pulisci elenco").clicked() {
                    self.jobs.clear();
                }
            }
        });
    }

    fn queue(&self, ui: &mut egui::Ui) {
        if self.jobs.is_empty() {
            return;
        }
        card(ui, |ui| {
            ui.label(RichText::new("Download").strong());
            for j in &self.jobs {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let col = match j.state {
                        JobState::Waiting => ui.visuals().weak_text_color(),
                        JobState::Running => ACCENT,
                        JobState::Done => OK,
                        JobState::Failed => ERR,
                    };
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(12.0, 14.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 5.0, col);
                    ui.add(egui::Label::new(&j.label).truncate());
                });
                if j.state == JobState::Running {
                    ui.add(egui::ProgressBar::new(j.pct).desired_height(8.0));
                }
                if !j.detail.is_empty() {
                    let col = if j.state == JobState::Failed {
                        ERR
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    ui.label(RichText::new(&j.detail).small().color(col));
                }
            }
        });
    }

    fn log_view(&self, ui: &mut egui::Ui) {
        if self.log.is_empty() {
            return;
        }
        ui.add_space(6.0);
        ui.collapsing("Registro dettagliato", |ui| {
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for l in &self.log {
                        ui.label(RichText::new(l).monospace().small());
                    }
                });
        });
    }

    fn footer(&self, ui: &mut egui::Ui) {
        ui.separator();
        let yt = self.tools.ytdlp.as_deref().unwrap_or("…");
        let ok = |b: bool| if b { "ok" } else { "manca" };
        ui.label(
            RichText::new(format!(
                "Grabbit {} · yt-dlp {} · ffmpeg {} · Deno {}",
                selfupdate::CURRENT,
                yt,
                ok(self.tools.ffmpeg.is_some()),
                ok(self.tools.deno.is_some()),
            ))
            .small()
            .color(ui.visuals().weak_text_color()),
        );
        ui.label(
            RichText::new("Usalo solo per contenuti che hai il diritto di scaricare, nel rispetto del copyright e dei termini dei servizi.")
                .small()
                .color(ui.visuals().weak_text_color()),
        );
    }
}

// ───────────────────────────── utilità ─────────────────────────────

fn label_of<'a>(list: &'a [(&'a str, &'a str)], key: &str) -> &'a str {
    list.iter()
        .find(|(k, _)| *k == key)
        .map(|(_, l)| *l)
        .unwrap_or("—")
}

fn open_folder(p: &std::path::Path) {
    let cmd = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd).arg(p).spawn();
}

fn load_logo(ctx: &egui::Context) -> egui::TextureHandle {
    let img = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .expect("icona incorporata non valida")
        .to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    ctx.load_texture("logo", color, egui::TextureOptions::LINEAR)
}

fn configure_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.visuals.selection.bg_fill = ACCENT;
        style.visuals.hyperlink_color = ACCENT;
        let r = egui::CornerRadius::same(8);
        style.visuals.widgets.noninteractive.corner_radius = r;
        style.visuals.widgets.inactive.corner_radius = r;
        style.visuals.widgets.hovered.corner_radius = r;
        style.visuals.widgets.active.corner_radius = r;
    });
}
