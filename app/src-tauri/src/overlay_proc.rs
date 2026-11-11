//! Starts the overlay program and writes messages to it.

use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};

pub struct OverlayProcess {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl OverlayProcess {
    /// The overlay executable is expected next to this app's executable.
    pub fn spawn(log_file: &std::path::Path) -> Result<Self, String> {
        let name = if cfg!(windows) { "riftclip-overlay.exe" } else { "riftclip-overlay" };
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join(name)))
            .filter(|p| p.is_file())
            .ok_or_else(|| format!("{name} was not found next to the app"))?;
        let mut cmd = Command::new(exe);
        cmd.arg("--log").arg(log_file).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        let stdin = child.stdin.take();
        Ok(OverlayProcess { child, stdin })
    }

    /// Returns false if the overlay is no longer there.
    pub fn send(&mut self, message: &proto::Message) -> bool {
        let Some(stdin) = self.stdin.as_mut() else { return false };
        writeln!(stdin, "{}", message.to_line()).and_then(|_| stdin.flush()).is_ok()
    }
}

impl Drop for OverlayProcess {
    fn drop(&mut self) {
        // Closing its input makes the overlay exit on its own; kill is the backstop.
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
