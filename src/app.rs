//! egui front end: settings, status and a stop button; start only via the global hotkey.

use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

use eframe::egui;

use crate::clicker::{ClickConfig, ClickerHandle, MouseButton, WorkerFailure, WorkerGone};
use crate::hotkey::{HotkeyController, ToggleKey};

const MIN_INTERVAL_MS: u64 = 1;
const MAX_INTERVAL_MS: u64 = 600_000;
const DEFAULT_INTERVAL_MS: u64 = 100;
const DEFAULT_KEY: ToggleKey = ToggleKey::F6;
const STATUS_REFRESH: Duration = Duration::from_millis(100);
const EVENT_QUEUE_CAPACITY: usize = 32;

enum UiEvent {
    HotkeyPressed(u32),
    ClickerFailed(WorkerFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunState {
    Idle,
    Running,
}

enum Hotkeys {
    Available(HotkeyController),
    Unavailable(global_hotkey::Error),
}

pub struct App {
    button: MouseButton,
    interval_ms: u64,
    toggle_key: ToggleKey,
    hotkeys: Hotkeys,
    clicker: ClickerHandle,
    state: RunState,
    events: Receiver<UiEvent>,
    error: Option<String>,
}

impl App {
    pub fn new(ctx: &egui::Context) -> std::io::Result<Self> {
        let (sender, events) = mpsc::sync_channel(EVENT_QUEUE_CAPACITY);
        let clicker = ClickerHandle::spawn(notifier(ctx, &sender, UiEvent::ClickerFailed))?;
        let hotkeys = match HotkeyController::new(
            DEFAULT_KEY,
            notifier(ctx, &sender, UiEvent::HotkeyPressed),
        ) {
            Ok(controller) => Hotkeys::Available(controller),
            Err(err) => Hotkeys::Unavailable(err),
        };
        Ok(Self {
            button: MouseButton::Left,
            interval_ms: DEFAULT_INTERVAL_MS,
            toggle_key: DEFAULT_KEY,
            hotkeys,
            clicker,
            state: RunState::Idle,
            events,
            error: None,
        })
    }

    fn handle_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                UiEvent::HotkeyPressed(id) => {
                    if self.bound_hotkey_id() == Some(id) {
                        self.toggle();
                    }
                }
                UiEvent::ClickerFailed(failure) => {
                    self.state = RunState::Idle;
                    self.error = Some(failure.to_string());
                }
            }
        }
    }

    fn bound_hotkey_id(&self) -> Option<u32> {
        match &self.hotkeys {
            Hotkeys::Available(controller) => controller.bound_id(),
            Hotkeys::Unavailable(_) => None,
        }
    }

    fn toggle(&mut self) {
        match self.state {
            RunState::Idle => self.start(),
            RunState::Running => self.stop(),
        }
    }

    fn start(&mut self) {
        let config = ClickConfig {
            button: self.button,
            interval: Duration::from_millis(self.interval_ms),
        };
        match self.clicker.start(config) {
            Ok(()) => {
                self.state = RunState::Running;
                self.error = None;
            }
            Err(WorkerGone) => self.error = Some(worker_gone_message()),
        }
    }

    fn stop(&mut self) {
        self.state = RunState::Idle;
        if let Err(WorkerGone) = self.clicker.stop() {
            self.error = Some(worker_gone_message());
        }
    }

    fn rebind_hotkey(&mut self) {
        let Hotkeys::Available(controller) = &mut self.hotkeys else {
            return;
        };
        self.error = match controller.rebind(self.toggle_key) {
            Ok(()) => None,
            Err(err) => Some(format!(
                "Не удалось назначить {}: {err}",
                self.toggle_key.label()
            )),
        };
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        let running = self.state == RunState::Running;
        ui.heading("Автокликер");
        ui.add_space(8.0);
        ui.add_enabled_ui(!running, |ui| self.draw_settings(ui));
        ui.add_space(8.0);
        ui.separator();
        ui.label(self.status_text());
        ui.small(self.hotkey_hint());
        if running {
            ui.add_space(4.0);
            if ui.button("Стоп").clicked() {
                self.stop();
            }
        }
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }

    fn draw_settings(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("settings")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Кнопка мыши");
                egui::ComboBox::from_id_salt("button")
                    .selected_text(self.button.label())
                    .show_ui(ui, |ui| {
                        for button in MouseButton::ALL {
                            ui.selectable_value(&mut self.button, button, button.label());
                        }
                    });
                ui.end_row();

                ui.label("Интервал");
                ui.add(
                    egui::DragValue::new(&mut self.interval_ms)
                        .range(MIN_INTERVAL_MS..=MAX_INTERVAL_MS)
                        .speed(1.0)
                        .suffix(" мс"),
                );
                ui.end_row();

                ui.label("Горячая клавиша");
                self.draw_hotkey_picker(ui);
                ui.end_row();
            });
    }

    fn draw_hotkey_picker(&mut self, ui: &mut egui::Ui) {
        if let Hotkeys::Unavailable(err) = &self.hotkeys {
            ui.colored_label(ui.visuals().error_fg_color, format!("недоступна: {err}"));
            return;
        }
        let before = self.toggle_key;
        egui::ComboBox::from_id_salt("hotkey")
            .selected_text(self.toggle_key.label())
            .show_ui(ui, |ui| {
                for key in ToggleKey::ALL {
                    ui.selectable_value(&mut self.toggle_key, key, key.label());
                }
            });
        if self.toggle_key != before {
            self.rebind_hotkey();
        }
    }

    fn status_text(&self) -> String {
        match self.state {
            RunState::Idle => "Остановлен".to_owned(),
            RunState::Running => format!("Работает, кликов: {}", self.clicker.clicks()),
        }
    }

    fn hotkey_hint(&self) -> String {
        match &self.hotkeys {
            Hotkeys::Available(controller) if controller.bound_id().is_some() => {
                format!("Старт/стоп: {}", self.toggle_key.label())
            }
            Hotkeys::Available(_) => "Горячая клавиша не назначена".to_owned(),
            Hotkeys::Unavailable(_) => "Горячие клавиши недоступны".to_owned(),
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_events();
        if self.state == RunState::Running {
            ctx.request_repaint_after(STATUS_REFRESH);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| self.draw(ui));
    }
}

fn worker_gone_message() -> String {
    "Поток кликов завершился, перезапустите программу".to_owned()
}

/// Builds a callback that queues an event for the UI thread and wakes it.
fn notifier<T: Send + 'static>(
    ctx: &egui::Context,
    sender: &SyncSender<UiEvent>,
    wrap: fn(T) -> UiEvent,
) -> impl Fn(T) + Send + Sync + 'static {
    let ctx = ctx.clone();
    let sender = sender.clone();
    move |payload| {
        // A full queue means the UI thread is not draining; dropping the event
        // beats blocking the worker or the window message loop.
        let _ = sender.try_send(wrap(payload));
        ctx.request_repaint();
    }
}
