use combine::{stream::position, EasyParser, StreamOnce};
use holochain_conductor_client::{AdminWebsocket, AppWebsocket, ZomeCall};
use holochain_types::{
    app::AppBundleSource,
    dna::{AgentPubKey, DnaBundle},
    prelude::{CellId, InstallAppBundlePayload},
};
use holochain_zome_types::zome_io::ExternIO;
use scrawl;
use serde_json;
use std::{
    error, fs, io,
    path::{Path, PathBuf},
    sync::mpsc::Sender,
};
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
use xdg;

use common::CreateInterchangeEntryInput;
use rep_lang_concrete_syntax::parse::expr;
use rep_lang_core::abstract_syntax::Expr;
use rep_lang_runtime::{env::Env, infer::infer_expr, types::Scheme};

const APP_ID: &str = "rep_interchange";

#[allow(dead_code)]
mod event;
use event::{Event, Events, HcInfo};

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
    hc_info: Option<HcInfo>,
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
            hc_info: None,
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

    let xdg_dirs = xdg::BaseDirectories::with_prefix("rlp").unwrap();
    let mut app = App::new();

    {
        let send = app.event_sender.clone();
        tokio::task::spawn(async move {
            let app_ws = AppWebsocket::connect("ws://127.0.0.1:9999".into())
                .await
                .expect("connect to succeed");
            let mut admin_ws = AdminWebsocket::connect("ws://127.0.0.1:9000".into())
                .await
                .expect("connect to succeed");
            // TODO address failure mode where file exists but does not deserialize
            let agent_pk = match xdg_dirs.find_data_file("agent_pk") {
                Some(agent_pk_path) => {
                    eprintln!("found agent_pk file, loading.");
                    let agent_pk_str = fs::read_to_string(agent_pk_path).unwrap();
                    let agent_pk: AgentPubKey = serde_json::from_str(&agent_pk_str).unwrap();
                    agent_pk
                }
                None => {
                    eprintln!("no agent_pk file found, generating from holochain/lair.");
                    let agent_pk = admin_ws.generate_agent_pub_key().await.unwrap();
                    let agent_pk_pathbuf = xdg_dirs.place_data_file("agent_pk").unwrap();
                    let agent_pk_str = serde_json::to_string(&agent_pk).unwrap();
                    fs::write(agent_pk_pathbuf.as_path(), agent_pk_str).unwrap();
                    eprintln!("wrote key file: {:?}", agent_pk_pathbuf);
                    agent_pk
                }
            };
            let hc_info = HcInfo {
                admin_ws: admin_ws.clone(),
                app_ws,
                agent_pk: agent_pk.clone(),
            };
            send.send(Event::HcInfo(hc_info)).expect("send to succeed");

            let pathbuf = PathBuf::from("./happs/rep_interchange/rep_interchange.happ");
            let iabp = InstallAppBundlePayload {
                source: AppBundleSource::Path(pathbuf),
                agent_key: agent_pk,
                installed_app_id: Some(APP_ID.into()),
                membrane_proofs: Default::default(),
                uid: None,
            };
            let _app_info = admin_ws.install_app_bundle(iabp).await.unwrap();
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
                match (&app.expr_state, &mut app.hc_info) {
                    (ExprState::Invalid(_), _) => {} // invalid expr
                    (_, None) => {}                  // no hc_ws client
                    (ExprState::Valid(_sc, expr), Some(hc_info)) => {
                        let apps_interfaces = hc_info.admin_ws.list_app_interfaces().await;
                        let apps = hc_info.admin_ws.list_apps(None).await;
                        eprintln!("apps_interfaces: {:?}", apps_interfaces);
                        eprintln!("apps: {:?}", apps);
                        let input: CreateInterchangeEntryInput = CreateInterchangeEntryInput {
                            expr: expr.clone(),
                            args: Vec::new(),
                        };
                        let payload = ExternIO::encode(input).unwrap();
                        let cell_id = {
                            // TODO consider loading the DNA async, at TUI launch time.
                            let path = Path::new("./happs/rep_interchange/rep_interchange.dna");
                            let bundle = DnaBundle::read_from_file(path).await.unwrap();
                            let (_dna_file, dna_hash) =
                                bundle.into_dna_file(None, None).await.unwrap();
                            CellId::new(dna_hash, hc_info.agent_pk.clone())
                        };
                        let _zc = ZomeCall {
                            cell_id,
                            zome_name: "interpreter".into(),
                            fn_name: "create_interchange_entry".into(),
                            payload,
                            cap: None,
                            provenance: hc_info.agent_pk.clone(),
                        };
                        // // TODO \/ we have a problem here: CellMissing
                        // let result = hc_info.app_ws.zome_call(zc).await.unwrap();
                        // let ie_hash: HeaderHash = result.decode().unwrap();
                        // app.hc_response = format!("create: ie_hash: {:?}", ie_hash);
                    }
                }
            }
            Event::HcInfo(hc_info) => {
                app.hc_info = Some(hc_info);
                app.hc_response = "hc_info: connected".into();
            }
            _ => {}
        }
    }
    Ok(())
}
