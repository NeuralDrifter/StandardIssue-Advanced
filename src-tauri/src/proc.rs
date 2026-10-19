// Michael Burgus — https://github.com/NeuralDrifter
// Child processes started from the GUI

use std::ffi::OsStr;

/// A command that opens no console window on Windows (the app itself has none to share).
pub fn command(program: impl AsRef<OsStr>) -> tokio::process::Command {
    #[allow(unused_mut)]
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}
