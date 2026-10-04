// What `tauri::AppHandle` was to the backend modules: a cheap, cloneable handle
// for sending events to the UI thread and reaching the few pieces of shared
// state (settings, pending permission requests, the chat history).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::claude::Chat;
use crate::files::DroppedFile;
use crate::integrations::IntegrationUpdate;
use crate::pipe::Pending;
use crate::settings::Settings;

/// Posted to the island window whenever the queue gets something new.
pub const WM_COUCOU_EVENT: u32 = 0x8000 + 1; // WM_APP + 1

pub enum Event {
    Hook(Value),
    Integration(IntegrationUpdate),
    ChatReply(Result<String, String>),
    Ingested(Result<DroppedFile, String>),
}

struct Inner {
    queue: Mutex<VecDeque<Event>>,
    hwnd: AtomicIsize,
    settings: Mutex<Settings>,
    pending: Pending,
    chat: Chat,
}

#[derive(Clone)]
pub struct AppHandle(Arc<Inner>);

impl AppHandle {
    pub fn new(settings: Settings) -> Self {
        Self(Arc::new(Inner {
            queue: Mutex::new(VecDeque::new()),
            hwnd: AtomicIsize::new(0),
            settings: Mutex::new(settings),
            pending: Pending::default(),
            chat: Chat::default(),
        }))
    }

    pub fn set_hwnd(&self, hwnd: HWND) {
        self.0.hwnd.store(hwnd.0 as isize, Ordering::Relaxed);
    }

    pub fn push(&self, e: Event) {
        self.0.queue.lock().unwrap().push_back(e);
        let h = self.0.hwnd.load(Ordering::Relaxed);
        if h != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(h as *mut _)), WM_COUCOU_EVENT, WPARAM(0), LPARAM(0));
            }
        }
    }

    pub fn drain(&self) -> Vec<Event> {
        self.0.queue.lock().unwrap().drain(..).collect()
    }

    pub fn emit_hook(&self, payload: Value) {
        self.push(Event::Hook(payload));
    }

    pub fn emit_integration(&self, update: IntegrationUpdate) {
        self.push(Event::Integration(update));
    }

    pub fn settings(&self) -> Settings {
        self.0.settings.lock().unwrap().clone()
    }

    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) -> Settings {
        let mut s = self.0.settings.lock().unwrap();
        f(&mut s);
        s.clone()
    }

    pub fn pending(&self) -> &Pending {
        &self.0.pending
    }

    pub fn chat(&self) -> &Chat {
        &self.0.chat
    }
}
