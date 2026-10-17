//! Desktop notifications about clicker state, shown from a dedicated thread.

use std::fmt;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;

use notify_rust::Notification;

use crate::clicker::{ClickConfig, WorkerFailure};

/// Notices wait here while the OS shows earlier ones; a longer backlog means
/// the OS call is stuck, and the UI thread must not pile up behind it.
const QUEUE_CAPACITY: usize = 4;

pub enum Notice {
    Started(ClickConfig),
    Stopped,
    Failed(WorkerFailure),
}

impl Notice {
    fn summary(&self) -> &'static str {
        match self {
            Self::Started(_) => "Автокликер запущен",
            Self::Stopped | Self::Failed(_) => "Автокликер остановлен",
        }
    }

    fn body(&self) -> String {
        match self {
            Self::Started(config) => format!(
                "{} кнопка, интервал {} мс",
                config.button.label(),
                config.interval.as_millis()
            ),
            Self::Stopped => String::new(),
            Self::Failed(failure) => failure.to_string(),
        }
    }
}

/// Why a notice could not be queued; the notice is dropped either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueError {
    /// The notification thread has exited.
    Gone,
    /// Earlier notices are still being shown.
    Full,
}

impl fmt::Display for QueueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gone => f.write_str("поток уведомлений завершился"),
            Self::Full => f.write_str("очередь уведомлений переполнена"),
        }
    }
}

pub struct NotifierHandle {
    notices: SyncSender<Notice>,
}

impl NotifierHandle {
    /// `on_failure` runs on the notification thread.
    pub fn spawn(
        on_failure: impl Fn(notify_rust::error::Error) + Send + 'static,
    ) -> std::io::Result<Self> {
        let (notices, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        thread::Builder::new()
            .name("notifier".to_owned())
            .spawn(move || run(&receiver, &on_failure))?;
        Ok(Self { notices })
    }

    /// Never blocks: the caller is the UI thread.
    pub fn show(&self, notice: Notice) -> Result<(), QueueError> {
        self.notices.try_send(notice).map_err(|err| match err {
            TrySendError::Full(_) => QueueError::Full,
            TrySendError::Disconnected(_) => QueueError::Gone,
        })
    }
}

/// Exits when every [`NotifierHandle`] has been dropped.
fn run(notices: &Receiver<Notice>, on_failure: &dyn Fn(notify_rust::error::Error)) {
    for notice in notices {
        if let Err(err) = show(&notice) {
            on_failure(err);
        }
    }
}

fn show(notice: &Notice) -> notify_rust::error::Result<()> {
    Notification::new()
        .summary(notice.summary())
        .body(&notice.body())
        .show()
        // The handle only serves to wait for the user's reaction to the toast.
        .map(drop)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::clicker::MouseButton;

    #[test]
    fn started_notice_describes_config() {
        let notice = Notice::Started(ClickConfig {
            button: MouseButton::Right,
            interval: Duration::from_millis(250),
        });
        assert_eq!(notice.summary(), "Автокликер запущен");
        assert_eq!(notice.body(), "Правая кнопка, интервал 250 мс");
    }

    #[test]
    fn stopped_notice_has_empty_body() {
        assert_eq!(Notice::Stopped.summary(), "Автокликер остановлен");
        assert_eq!(Notice::Stopped.body(), "");
    }
}
