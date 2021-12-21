use combine::{stream::position, EasyParser, StreamOnce};
use holo_hash::EntryHash;
use holochain_conductor_client::{AdminWebsocket, AppWebsocket, ZomeCall};
use holochain_types::{
    app::AppBundleSource,
    dna::DnaBundle,
    prelude::{CellId, InstallAppBundlePayload},
};
use holochain_zome_types::zome_io::ExternIO;
use std::{
    error, io,
    path::{Path, PathBuf},
    sync::mpsc::Sender,
};
use termion::{event::Key, input::MouseTerminal, raw::IntoRawMode, screen::AlternateScreen};
use tui::{
    backend::TermionBackend,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Span, Spans, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};

use common::{CreateInterchangeEntryInput, InterchangeEntry, InterchangeOperand};
use rep_lang_concrete_syntax::parse::expr;
use rep_lang_core::abstract_syntax::Expr;
use rep_lang_runtime::{
    env::Env,
    infer::infer_expr,
    types::{Scheme, Type},
};

mod event;
use event::{Event, Events, HcInfo};

const APP_ID: &str = "rep_interchange";

#[derive(Debug, Clone)]
pub enum ExprState {
    Valid(ValidExprState),
    Invalid(String),
}

#[derive(Debug, Clone)]
pub struct ValidExprState {
    sc: Scheme,
    expr: Expr,
    /// any IEs we have already selected for `expr` to be applied to. Vec
    /// ordering is the order in which they will be applied.
    args: Vec<(EntryHash, InterchangeEntry)>,
    /// IEs which have not yet been selected for application, but are
    /// candidates (meaning that `expr` must be a closure & `sc` must have a
    /// toplevel `TArr`. and the closure argument's `Scheme` unifies with all
    /// of these candidates individually).
    next_application_candidates: Vec<(EntryHash, InterchangeEntry)>,
}

impl ExprState {
    fn is_valid(&self) -> bool {
        match self {
            ExprState::Valid(_) => true,
            ExprState::Invalid(_) => false,
        }
    }
}

enum ViewState {
    Viewer(Vec<InterchangeEntry>),
    Creator,
}

impl ViewState {
    fn toggle(&self) -> ViewState {
        match &self {
            ViewState::Viewer(_) => ViewState::Creator,
            ViewState::Creator => ViewState::Viewer(vec![]),
        }
    }

    fn is_creator(&self) -> bool {
        matches!(self, ViewState::Creator)
    }

    fn is_viewer(&self) -> bool {
        matches!(self, ViewState::Viewer(_))
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
    /// when the TUI has control of the screen & input, this should always be
    /// `Some`.
    opt_events: Option<Events>,
    event_sender: Sender<Event>,
    hc_info: Option<HcInfo>,
    hc_responses: Vec<String>,
    view_state: ViewState,
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
            hc_responses: vec!["not connected".into()],
            view_state: ViewState::Creator,
        }
    }

    fn log_hc_response(&mut self, s: String) {
        self.hc_responses.insert(0, s);
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
        let app_ws = AppWebsocket::connect("ws://127.0.0.1:9999".into())
            .await
            .expect("connect to succeed");
        let mut admin_ws = AdminWebsocket::connect("ws://127.0.0.1:9000".into())
            .await
            .expect("connect to succeed");
        let agent_pk = admin_ws.generate_agent_pub_key().await.unwrap();
        let dna_hash = {
            let path = Path::new("./happs/rep_interchange/rep_interchange.dna");
            let bundle = DnaBundle::read_from_file(path).await.unwrap();
            let (_dna_file, dna_hash) = bundle.into_dna_file(None, None).await.unwrap();
            dna_hash
        };
        let hc_info = HcInfo {
            admin_ws: admin_ws.clone(),
            app_ws,
            agent_pk: agent_pk.clone(),
            dna_hash,
        };
        app.event_sender
            .send(Event::HcInfo(hc_info))
            .expect("send to succeed");

        let pathbuf = PathBuf::from("./happs/rep_interchange/rep_interchange.happ");
        let iabp = InstallAppBundlePayload {
            source: AppBundleSource::Path(pathbuf),
            agent_key: agent_pk,
            installed_app_id: Some(APP_ID.into()),
            membrane_proofs: Default::default(),
            uid: None,
        };
        let _app_info = admin_ws.install_app_bundle(iabp).await.unwrap();
        let _enable_app_response = admin_ws.enable_app(APP_ID.into()).await.unwrap();
    }

    loop {
        // draw UI
        terminal.draw(|f| match &app.view_state {
            ViewState::Creator => {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(1)
                    .constraints(
                        [
                            Constraint::Length(1),
                            Constraint::Length(25),
                            Constraint::Min(1),
                            Constraint::Length(6),
                        ]
                        .as_ref(),
                    )
                    .split(f.size());

                let mut default_commands = vec![
                    Span::raw("press "),
                    Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(" to exit, "),
                    Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(" to switch to viewer, "),
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

                let hc_responses = app
                    .hc_responses
                    .iter()
                    .map(|resp| "- ".to_string() + resp)
                    .collect::<Vec<String>>()
                    .join("\n");
                let app_info = Paragraph::new(hc_responses).style(Style::default()).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("holochain responses (newest first)"),
                );
                f.render_widget(app_info, chunks[3]);
            }

            ViewState::Viewer(ie_s) => {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(1)
                    .constraints([Constraint::Length(1), Constraint::Min(1)].as_ref())
                    .split(f.size());

                let help_spans = vec![
                    Span::raw("press "),
                    Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(" to exit, "),
                    Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(" to switch to creator."),
                ];
                let help_msg = Paragraph::new(Text::from(Spans::from(help_spans)));
                f.render_widget(help_msg, chunks[0]);

                let rendered_ie_s: String = ie_s
                    .iter()
                    .map(|ie| format!("{:?}", ie))
                    .collect::<Vec<String>>()
                    .join("\n\n");

                let block = Paragraph::new(rendered_ie_s)
                    .wrap(Wrap { trim: false })
                    .style(Style::default())
                    .block(Block::default().borders(Borders::ALL).title("viewer"));
                f.render_widget(block, chunks[1]);
            }
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
            Event::Input(Key::Char('e')) if app.view_state.is_creator() => {
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
                                Err(err) => ExprState::Invalid(format!("type error: {:?}", err)),
                                Ok(sc) => {
                                    let _opt_arg_sc = match &sc {
                                        Scheme(tvs, Type::TArr(arg, _)) => {
                                            Some(Scheme(tvs.clone(), *arg.clone()))
                                        }
                                        _ => None,
                                    };
                                    ExprState::Valid(ValidExprState {
                                        sc,
                                        expr,
                                        args: vec![],
                                        next_application_candidates: vec![],
                                    })
                                }
                            }
                        }
                    }
                };
                app.expr_state = st;
            }
            Event::Input(Key::Char('c')) if app.view_state.is_creator() => {
                match (&app.expr_state, &mut app.hc_info) {
                    (ExprState::Invalid(_), _) => {} // invalid expr
                    (_, None) => {}                  // no hc_ws client
                    (ExprState::Valid(ves), Some(hc_info)) => {
                        let args = ves
                            .args
                            .iter()
                            .map(|(e_hash, _ie)| {
                                InterchangeOperand::InterchangeOperand(e_hash.clone())
                            })
                            .collect();
                        let input: CreateInterchangeEntryInput = CreateInterchangeEntryInput {
                            expr: ves.expr.clone(),
                            args,
                        };
                        let payload = ExternIO::encode(input).unwrap();
                        let cell_id =
                            CellId::new(hc_info.dna_hash.clone(), hc_info.agent_pk.clone());
                        let zc = ZomeCall {
                            cell_id,
                            zome_name: "interpreter".into(),
                            fn_name: "create_interchange_entry".into(),
                            payload,
                            cap_secret: None,
                            provenance: hc_info.agent_pk.clone(),
                        };
                        let result = hc_info.app_ws.zome_call(zc).await.unwrap();
                        let ie_hash: EntryHash = result.decode().unwrap();
                        app.log_hc_response(format!("create: ie_hash: {:?}", ie_hash));
                    }
                }
            }
            Event::Input(Key::Char('\t')) => {
                app.view_state = app.view_state.toggle();
                // TODO this cloning could maybe be eliminated
                let mut hc_info = app.hc_info.clone().unwrap();

                if app.view_state.is_viewer() {
                    let opt_sc: Option<Scheme> = None;
                    let payload = ExternIO::encode(opt_sc).unwrap();
                    let cell_id = CellId::new(hc_info.dna_hash.clone(), hc_info.agent_pk.clone());
                    let zc = ZomeCall {
                        cell_id,
                        zome_name: "interpreter".into(),
                        fn_name: "get_interchange_entries_which_unify".into(),
                        payload,
                        cap_secret: None,
                        provenance: hc_info.agent_pk.clone(),
                    };
                    let result = hc_info.app_ws.zome_call(zc).await.unwrap();
                    let ie_s: Vec<InterchangeEntry> = result.decode().unwrap();
                    app.event_sender
                        .send(Event::GetIes(ie_s))
                        .expect("send to succeed");
                }
            }
            Event::HcInfo(hc_info) => {
                app.hc_info = Some(hc_info);
                app.log_hc_response("hc_info: connected".into());
            }
            Event::GetIes(ie_s) => {
                app.view_state = ViewState::Viewer(ie_s);
                app.log_hc_response("got IEs".into());
            }
            _ => {}
        }
    }
    Ok(())
}
