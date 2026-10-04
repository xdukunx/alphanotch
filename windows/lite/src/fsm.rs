// Island open/close FSM — port of island/fsm.ts (IslandStateMachine.swift).
// No window, no clock of its own: it reports transitions and holds deadlines the
// host checks on every tick.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fsm {
    Hidden,
    Petit,
    Home,
    Coucou,
}

pub struct Machine {
    pub state: Fsm,
    /// home → petit delay, seconds.
    pub home_to_petit: f32,
    /// petit → hidden delay, seconds.
    pub petit_to_hidden: f32,
    pub greet_auto_collapse: f32,
    pub greet_hover_collapse: f32,
    /// An alert waiting for an answer stays open, even when the mouse leaves.
    pub pinned: bool,

    petit_hide: Option<f32>,
    home_collapse: Option<f32>,
    greet_collapse: Option<f32>,
    transitions: Vec<(Fsm, Fsm)>,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    pub fn new() -> Self {
        Self {
            state: Fsm::Hidden,
            home_to_petit: 15.0,
            petit_to_hidden: 60.0,
            greet_auto_collapse: 0.6,
            greet_hover_collapse: 10.0,
            pinned: false,
            petit_hide: None,
            home_collapse: None,
            greet_collapse: None,
            transitions: Vec::new(),
        }
    }

    pub fn take_transitions(&mut self) -> Vec<(Fsm, Fsm)> {
        std::mem::take(&mut self.transitions)
    }

    /// True while a deadline is pending, i.e. the host must keep ticking.
    pub fn has_deadline(&self) -> bool {
        self.petit_hide.is_some() || self.home_collapse.is_some() || self.greet_collapse.is_some()
    }

    pub fn launch(&mut self) {
        self.cancel_timers();
        self.transition(Fsm::Coucou);
    }

    pub fn mouse_entered(&mut self, now: f32) {
        match self.state {
            Fsm::Hidden => {
                self.cancel_timers();
                self.transition(Fsm::Petit);
            }
            Fsm::Petit => self.petit_hide = None,
            Fsm::Home => self.home_collapse = None,
            Fsm::Coucou => self.schedule_greet(now, self.greet_hover_collapse),
        }
    }

    pub fn mouse_left(&mut self, now: f32) {
        match self.state {
            Fsm::Hidden => {}
            Fsm::Petit => self.schedule_petit_hide(now),
            Fsm::Home => self.schedule_home_collapse(now),
            Fsm::Coucou => {
                self.greet_collapse = None;
                self.transition(Fsm::Petit);
            }
        }
    }

    pub fn click(&mut self) {
        if self.state != Fsm::Petit {
            return;
        }
        self.cancel_timers();
        self.transition(Fsm::Home);
    }

    /// Greeting animation finished. Doesn't override a running hover timer.
    pub fn greet_complete(&mut self, now: f32) {
        if self.state != Fsm::Coucou {
            return;
        }
        if self.greet_collapse.is_none() {
            self.schedule_greet(now, self.greet_auto_collapse);
        }
    }

    /// Non-alert work event: show compact from hidden.
    pub fn reveal(&mut self, now: f32) {
        if self.state != Fsm::Hidden {
            return;
        }
        self.cancel_timers();
        self.transition(Fsm::Petit);
        self.schedule_petit_hide(now);
    }

    /// Alert or explicit request: open straight to expanded.
    pub fn force_home(&mut self) {
        self.cancel_timers();
        self.transition(Fsm::Home);
    }

    pub fn force_petit(&mut self) {
        self.cancel_timers();
        self.transition(Fsm::Petit);
    }

    pub fn force_hidden(&mut self) {
        self.cancel_timers();
        self.transition(Fsm::Hidden);
    }

    pub fn tick(&mut self, now: f32) {
        if let Some(at) = self.petit_hide {
            if now >= at {
                self.petit_hide = None;
                if self.state == Fsm::Petit {
                    self.transition(Fsm::Hidden);
                }
            }
        }
        if let Some(at) = self.home_collapse {
            if now >= at {
                self.home_collapse = None;
                if self.state == Fsm::Home {
                    self.transition(Fsm::Petit);
                }
            }
        }
        if let Some(at) = self.greet_collapse {
            if now >= at {
                self.greet_collapse = None;
                if self.state == Fsm::Coucou {
                    self.transition(Fsm::Petit);
                }
            }
        }
    }

    fn schedule_petit_hide(&mut self, now: f32) {
        self.petit_hide = Some(now + self.petit_to_hidden);
    }

    fn schedule_home_collapse(&mut self, now: f32) {
        self.home_collapse = None;
        if self.pinned {
            return;
        }
        self.home_collapse = Some(now + self.home_to_petit);
    }

    fn schedule_greet(&mut self, now: f32, delay: f32) {
        self.greet_collapse = Some(now + delay);
    }

    pub fn cancel_timers(&mut self) {
        self.petit_hide = None;
        self.home_collapse = None;
        self.greet_collapse = None;
    }

    fn transition(&mut self, next: Fsm) {
        if next == self.state {
            return;
        }
        let from = self.state;
        self.state = next;
        self.transitions.push((from, next));
    }
}
