use holo_hash::HeaderHash;
use std::io;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use termion::event::Key;
use termion::input::TermRead;

use common::SensemakerEntry;

pub enum Event<HI> {
    Input(Key),
    HcInfo(HI),
    ViewerSes(Vec<SensemakerEntry>),
    SelectorSes(Vec<(HeaderHash, SensemakerEntry)>),
}

/// A small event handler that wrap termion input and tick events. Each event
/// type is handled in its own thread and returned to a common `Receiver`
pub struct Events<HI> {
    rx: Receiver<Event<HI>>,
    #[allow(dead_code)]
    input_handle: thread::JoinHandle<()>,
}

impl<HI: 'static + std::marker::Send> Events<HI> {
    pub fn mk() -> (Events<HI>, Sender<Event<HI>>) {
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

    pub fn next(&self) -> Result<Event<HI>, mpsc::RecvError> {
        self.rx.recv()
    }
}
