//! Global start/stop hotkey.

use global_hotkey::hotkey::{Code, HotKey};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleKey {
    F6,
    F7,
    F8,
    F9,
}

impl ToggleKey {
    pub const ALL: [Self; 4] = [Self::F6, Self::F7, Self::F8, Self::F9];

    pub fn label(self) -> &'static str {
        match self {
            Self::F6 => "F6",
            Self::F7 => "F7",
            Self::F8 => "F8",
            Self::F9 => "F9",
        }
    }

    fn code(self) -> Code {
        match self {
            Self::F6 => Code::F6,
            Self::F7 => Code::F7,
            Self::F8 => Code::F8,
            Self::F9 => Code::F9,
        }
    }
}

pub struct HotkeyController {
    manager: GlobalHotKeyManager,
    bound: Option<HotKey>,
}

impl HotkeyController {
    /// On Windows the manager must live on the thread that pumps the window
    /// message loop, so this is called from the eframe app creator.
    /// `on_press` receives the id of the pressed hotkey.
    pub fn new(
        key: ToggleKey,
        on_press: impl Fn(u32) + Send + Sync + 'static,
    ) -> Result<Self, global_hotkey::Error> {
        // The handler slot is a OnceLock that `send` initialises to `None` on first
        // use, so it must be set before anything can produce an event.
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.state == HotKeyState::Pressed {
                on_press(event.id);
            }
        }));
        let manager = GlobalHotKeyManager::new()?;
        let hotkey = HotKey::new(None, key.code());
        manager.register(hotkey)?;
        Ok(Self {
            manager,
            bound: Some(hotkey),
        })
    }

    pub fn bound_id(&self) -> Option<u32> {
        self.bound.map(|hotkey| hotkey.id())
    }

    /// On failure nothing stays bound; a later call retries from scratch.
    pub fn rebind(&mut self, key: ToggleKey) -> Result<(), global_hotkey::Error> {
        let next = HotKey::new(None, key.code());
        if let Some(current) = self.bound.take() {
            if current.id() == next.id() {
                self.bound = Some(current);
                return Ok(());
            }
            self.manager.unregister(current)?;
        }
        self.manager.register(next)?;
        self.bound = Some(next);
        Ok(())
    }
}
