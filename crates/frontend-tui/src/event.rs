use holo_hash::HeaderHash;
use holochain_conductor_client::{AdminWebsocket, AppWebsocket};
use holochain_types::dna::{AgentPubKey, DnaHash};
use std::io;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use termion::event::Key;
use termion::input::TermRead;

use common::InterchangeEntry;

pub enum Event {
    Input(Key),
    HcInfo(HcInfo),
    ViewerIes(Vec<InterchangeEntry>),
    SelectorIes(Vec<(HeaderHash, InterchangeEntry)>),
}

#[derive(Clone)]
pub struct HcInfo {
    pub admin_ws: AdminWebsocket,
    pub app_ws: AppWebsocket,
    pub agent_pk: AgentPubKey,
    pub dna_hash: DnaHash,
}

/// A small event handler that wrap termion input and tick events. Each event
/// type is handled in its own thread and returned to a common `Receiver`
pub struct Events {
    rx: Receiver<Event>,
    #[allow(dead_code)]
    input_handle: thread::JoinHandle<()>,
}

impl Events {
    pub fn mk() -> (Events, Sender<Event>) {
        let (tx, rx) = mpsc::channel();
        let input_handle = {
            let tx = tx.clone();
            thread::spawn(move || {
                let stdin = io::stdin();
                for key in stdin.keys().flatten() {
                    if let Err(_err) = tx.send(Event::Input(key)) {
                        // silently exit, otherwise output is distractingly visible
                        return;
                    }
                }
            })
        };
        (Events { rx, input_handle }, tx)
    }

    pub fn next(&self) -> Result<Event, mpsc::RecvError> {
        self.rx.recv()
    }
}
