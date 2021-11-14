use combine::{stream::position, EasyParser, StreamOnce};
use holo_hash::{HeaderHash, HoloHash};
use holochain_conductor_client::{AdminWebsocket, AppWebsocket, ZomeCall};
use holochain_types::{dna::DnaBundle, prelude::CellId};
use holochain_zome_types::zome_io::ExternIO;
use scrawl;
use std::{error, io, iter, path::Path, sync::mpsc::Sender};
use termion::{event::Key, input::MouseTerminal, raw::IntoRawMode, screen::AlternateScreen};
use tokio;
use tui::{
    backend::TermionBackend,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Span, Spans, Text},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};

use common::CreateInterchangeEntryInput;
use rep_lang_concrete_syntax::parse::expr;
use rep_lang_core::abstract_syntax::Expr;
use rep_lang_runtime::{env::Env, infer::infer_expr, types::Scheme};

#[allow(dead_code)]
mod event;
use event::{Event, Events};

#[derive(Debug, Clone)]
pub enum ExprState {
    Valid(Scheme, Expr),
    Invalid(String),
}

impl ExprState {
    fn is_valid(&self) -> bool {
        match self {
            ExprState::Valid(_, _) => true,
            ExprState::Invalid(_) => false,
        }
    }
}

struct App {
    /// the text which may parse to an `Expr`.
    expr_input: String,
    /// the result of parsing and typechecking `expr_input`.
    expr_state: ExprState,
    /// this is an Option so we can close the Events stream when we open EDITOR
    /// (by setting this field to `None` and thereby allowing the `Events` go
    /// out of scope and be collected).
    /// while the TUI has control of the screen & input, this should always be
    /// `Some`.
    opt_events: Option<Events>,
    event_sender: Sender<Event>,
    hc_ws_s: Option<(AdminWebsocket, AppWebsocket)>,
    hc_response: String,
}

impl App {
    fn new() -> App {
        let (events, event_sender) = Events::mk();
        App {
            expr_input: String::new(),
            expr_state: ExprState::Invalid("init".into()),
            opt_events: Some(events),
            event_sender,
            hc_ws_s: None,
            hc_response: "not connected".into(),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn error::Error>> {
    // terminal initialization
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    {
        let send = app.event_sender.clone();
        tokio::task::spawn(async move {
            let app_ws = AppWebsocket::connect("ws://127.0.0.1:9999".into())
                .await
                .expect("connect to succeed");
            let admin_ws = AdminWebsocket::connect("ws://127.0.0.1:9000".into())
                .await
                .expect("connect to succeed");
            send.send(Event::HcWs((admin_ws, app_ws)))
                .expect("send to succeed");
        });
    }

    loop {
        // draw UI
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints(
                    [
                        Constraint::Length(1),
                        Constraint::Length(25),
                        Constraint::Min(1),
                        Constraint::Length(4),
                    ]
                    .as_ref(),
                )
                .split(f.size());

            let mut default_commands = vec![
                Span::raw("press "),
                Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" to exit, "),
                Span::styled("e", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" to launch $EDITOR"),
            ];
            let mut valid_expr_commands = vec![
                Span::raw(", "),
                Span::styled("c", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" to create entry"),
            ];
            let msg = {
                if app.expr_state.is_valid() {
                    default_commands.append(&mut valid_expr_commands);
                }
                default_commands.push(Span::raw("."));
                default_commands
            };

            let style = Style::default().add_modifier(Modifier::RAPID_BLINK);
            let mut text = Text::from(Spans::from(msg));
            text.patch_style(style);
            let help_message = Paragraph::new(text);
            f.render_widget(help_message, chunks[0]);

            let expr_input = Paragraph::new(app.expr_input.as_ref())
                .style(Style::default())
                .block(Block::default().borders(Borders::ALL).title("expr input"));
            f.render_widget(expr_input, chunks[1]);

            let msgs = Paragraph::new(format!("{:?}", app.expr_state))
                .style(Style::default())
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("feedback on expr"),
                );
            f.render_widget(msgs, chunks[2]);

            let app_info = Paragraph::new(format!("{}", app.hc_response))
                .style(Style::default())
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("holochain response"),
                );
            f.render_widget(app_info, chunks[3]);
        })?;

        // handle input
        let evt = {
            match app.opt_events {
                None => panic!("impossible: logic error"),
                Some(ref itr) => itr.next()?,
            }
        };
        match evt {
            Event::Input(Key::Char('q')) => {
                terminal.clear().expect("clear to succeed");
                break;
            }
            Event::Input(Key::Char('e')) => {
                app.opt_events = None;
                terminal.clear().expect("clear to succeed");
                app.expr_input = scrawl::with(&app.expr_input)?;
                {
                    let (events, event_sender) = Events::mk();
                    app.opt_events = Some(events);
                    app.event_sender = event_sender;
                }
                terminal.clear().expect("clear to succeed");
                let st = match expr().easy_parse(position::Stream::new(&app.expr_input[..])) {
                    Err(err) => ExprState::Invalid(format!("parse error:\n\n{}\n", err)),
                    Ok((expr, extra_input)) => {
                        if extra_input.is_partial() {
                            ExprState::Invalid(format!(
                                "error: unconsumed input: {:?}",
                                extra_input
                            ))
                        } else {
                            match infer_expr(&Env::new(), &expr) {
                                Ok(sc) => ExprState::Valid(sc, expr),
                                Err(err) => ExprState::Invalid(format!("type error: {:?}", err)),
                            }
                        }
                    }
                };
                app.expr_state = st;
            }
            Event::Input(Key::Char('c')) => {
                match (&app.expr_state, &mut app.hc_ws_s) {
                    (ExprState::Invalid(_), _) => {} // invalid expr
                    (_, None) => {}                  // no hc_ws client
                    (ExprState::Valid(_sc, expr), Some((_, app_ws))) => {
                        let input: CreateInterchangeEntryInput = CreateInterchangeEntryInput {
                            expr: expr.clone(),
                            args: Vec::new(),
                        };
                        let payload = ExternIO::encode(input).unwrap();
                        let agent_pk_bytes: Vec<u8> = iter::repeat(1).take(36).collect();
                        let agent_pk = HoloHash::from_raw_36(agent_pk_bytes);
                        // TODO do all of this async, with jobs spawned at TUI
                        // start time, and store results in `App`
                        let cell_id = {
                            let path = Path::new("./happs/rep_interchange/rep_interchange.dna");
                            let bundle = DnaBundle::read_from_file(path).await.unwrap();
                            let (_dna_file, dna_hash) =
                                bundle.into_dna_file(None, None).await.unwrap();
                            CellId::new(dna_hash, agent_pk.clone())
                        };
                        let zc = ZomeCall {
                            cell_id,
                            zome_name: "interpreter".into(),
                            fn_name: "create_interchange_entry".into(),
                            payload,
                            cap: None,
                            provenance: agent_pk,
                        };
                        // TODO \/ we have a problem here: CellMissing
                        let result = app_ws.zome_call(zc).await.unwrap();
                        let ie_hash: HeaderHash = result.decode().unwrap();
                        app.hc_response = format!("create: ie_hash: {:?}", ie_hash);
                    }
                }
            }
            Event::HcWs(hc_ws_s) => {
                app.hc_ws_s = Some(hc_ws_s);
                app.hc_response = "hc_ws_s: connected".into();
            }
            _ => {}
        }
    }
    Ok(())
}
