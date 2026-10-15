//! Background thread that emits mouse clicks at a fixed interval.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use enigo::{Button, Direction, Enigo, InputError, Mouse, NewConError, Settings};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl MouseButton {
    pub const ALL: [Self; 3] = [Self::Left, Self::Right, Self::Middle];

    pub fn label(self) -> &'static str {
        match self {
            Self::Left => "Левая",
            Self::Right => "Правая",
            Self::Middle => "Средняя",
        }
    }

    fn into_enigo(self) -> Button {
        match self {
            Self::Left => Button::Left,
            Self::Right => Button::Right,
            Self::Middle => Button::Middle,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClickConfig {
    pub button: MouseButton,
    pub interval: Duration,
}

/// Reported once per failure; the worker goes idle after reporting.
#[derive(Debug)]
pub enum WorkerFailure {
    Init(NewConError),
    Click(InputError),
}

impl fmt::Display for WorkerFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Init(err) => write!(f, "Не удалось инициализировать ввод: {err}"),
            Self::Click(err) => write!(f, "Клик не выполнен: {err}"),
        }
    }
}

/// The worker thread has exited; no further clicks are possible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerGone;

#[derive(Clone, Copy)]
enum Command {
    Start(ClickConfig),
    Stop,
}

enum Phase {
    Idle,
    Running(ClickConfig),
}

pub struct ClickerHandle {
    commands: Sender<Command>,
    clicks: Arc<AtomicU64>,
}

impl ClickerHandle {
    /// `on_failure` runs on the worker thread.
    pub fn spawn(on_failure: impl Fn(WorkerFailure) + Send + 'static) -> std::io::Result<Self> {
        let (commands, receiver) = mpsc::channel();
        let clicks = Arc::new(AtomicU64::new(0));
        let worker_clicks = Arc::clone(&clicks);
        thread::Builder::new()
            .name("clicker".to_owned())
            .spawn(move || run(&receiver, &worker_clicks, &on_failure))?;
        Ok(Self { commands, clicks })
    }

    pub fn start(&self, config: ClickConfig) -> Result<(), WorkerGone> {
        self.commands
            .send(Command::Start(config))
            .map_err(|_| WorkerGone)
    }

    pub fn stop(&self) -> Result<(), WorkerGone> {
        self.commands.send(Command::Stop).map_err(|_| WorkerGone)
    }

    /// Clicks performed since the last start.
    pub fn clicks(&self) -> u64 {
        self.clicks.load(Ordering::Relaxed)
    }
}

struct Worker<'a> {
    enigo: Enigo,
    clicks: &'a AtomicU64,
    on_failure: &'a dyn Fn(WorkerFailure),
}

impl Worker<'_> {
    fn apply(&mut self, command: Command) -> Phase {
        match command {
            Command::Start(config) => {
                self.clicks.store(0, Ordering::Relaxed);
                self.click(config)
            }
            Command::Stop => Phase::Idle,
        }
    }

    fn click(&mut self, config: ClickConfig) -> Phase {
        match self
            .enigo
            .button(config.button.into_enigo(), Direction::Click)
        {
            Ok(()) => {
                self.clicks.fetch_add(1, Ordering::Relaxed);
                Phase::Running(config)
            }
            Err(err) => {
                (self.on_failure)(WorkerFailure::Click(err));
                Phase::Idle
            }
        }
    }
}

/// Exits when every [`ClickerHandle`] has been dropped.
fn run(commands: &Receiver<Command>, clicks: &AtomicU64, on_failure: &dyn Fn(WorkerFailure)) {
    let enigo = match Enigo::new(&Settings::default()) {
        Ok(enigo) => enigo,
        Err(err) => {
            on_failure(WorkerFailure::Init(err));
            return;
        }
    };
    let mut worker = Worker {
        enigo,
        clicks,
        on_failure,
    };
    let mut phase = Phase::Idle;
    loop {
        phase = match phase {
            Phase::Idle => match commands.recv() {
                Ok(command) => worker.apply(command),
                Err(_) => return,
            },
            Phase::Running(config) => match commands.recv_timeout(config.interval) {
                Ok(command) => worker.apply(command),
                Err(RecvTimeoutError::Timeout) => worker.click(config),
                Err(RecvTimeoutError::Disconnected) => return,
            },
        };
    }
}
