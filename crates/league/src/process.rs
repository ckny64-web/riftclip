//! Finds the game and client processes.

use crate::lcu::Credentials;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

pub struct ProcessWatcher {
    sys: System,
}

impl Default for ProcessWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessWatcher {
    pub fn new() -> Self {
        ProcessWatcher { sys: System::new().expect("process listing is available") }
    }

    fn refresh(&mut self) {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cmd(UpdateKind::OnlyIfNotSet),
        );
    }

    /// PID of the first process whose executable name equals `name` (case-insensitive).
    pub fn find(&mut self, name: &str) -> Option<u32> {
        self.refresh();
        self.sys
            .processes()
            .values()
            .find(|p| p.name().to_string_lossy().eq_ignore_ascii_case(name))
            .map(|p| p.pid().as_u32())
    }

    /// PID of the running game, if any.
    pub fn game_pid(&mut self) -> Option<u32> {
        self.find(crate::GAME_PROCESS)
    }

    /// Connection details for the League client, read from its command line.
    pub fn client_credentials(&mut self) -> Option<Credentials> {
        self.refresh();
        self.sys
            .processes()
            .values()
            .filter(|p| p.name().to_string_lossy().eq_ignore_ascii_case(crate::CLIENT_UX_PROCESS))
            .find_map(|p| {
                let args: Vec<String> = p.cmd().iter().map(|a| a.to_string_lossy().into_owned()).collect();
                Credentials::from_cmdline(&args)
            })
    }
}
