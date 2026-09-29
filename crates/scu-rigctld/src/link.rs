//! The radio side of the rigctld server, abstracted so the protocol logic can
//! be tested without a live session.

use scu_client::ScuHandle;

/// Send a CAT command to the radio (fire-and-forget).
pub trait RadioLink: Send + Sync {
    fn send(&self, command: &str);
}

impl RadioLink for ScuHandle {
    fn send(&self, command: &str) {
        self.send_cat(command);
    }
}
