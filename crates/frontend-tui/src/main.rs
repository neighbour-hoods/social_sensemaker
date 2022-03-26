use combine::{stream::position, EasyParser, StreamOnce};
use holo_hash::HeaderHash;
use holochain_conductor_client::{AdminWebsocket, AppWebsocket, ZomeCall};
use holochain_types::{
    app::AppBundleSource,
    dna::{AgentPubKey, DnaBundle, DnaHash},
    prelude::{CellId, InstallAppBundlePayload},
};
use holochain_zome_types::zome_io::ExternIO;
use pretty::RcDoc;
use std::{
    cmp, error, io,
    path::{Path, PathBuf},
    sync::mpsc::Sender,
};
use structopt::StructOpt;
use termion::{event::Key, input::MouseTerminal, raw::IntoRawMode, screen::AlternateScreen};
use tui::{
    backend::TermionBackend,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Span, Spans, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};

use common::{CreateSensemakerEntryInput, SensemakerEntry, SensemakerOperand};
use rep_lang_concrete_syntax::{parse::expr, pretty::ppr_expr, util::pretty::to_pretty};
use rep_lang_core::abstract_syntax::Expr;
use rep_lang_runtime::{
    env::Env,
    infer::{close_over, infer_expr, normalize, unifies, InferState},
    types::{Scheme, Type},
};

mod event;
use event::{Event, Events};

const APP_ID: &str = "rep_sensemaker";

#[derive(Debug, Clone)]
pub enum ExprState {
    Valid(ValidExprState),
    Invalid(String),
}

#[derive(Debug, Clone)]
pub struct ValidExprState {
    expr_sc: Scheme,
    expr: Expr,
    /// any SEs we have already selected for `expr` to be applied to. Vec
    /// ordering is the order in which they will be applied.
    args: Vec<(HeaderHash, SensemakerEntry)>,
    /// SEs which have not yet been selected for application, but are
    /// candidates (meaning that `expr` must be a closure & `expr_sc` must have a
    /// toplevel `TArr`. and the closure argument's `Scheme` unifies with all
    /// of these candidates individually).
    next_application_candidates: Vec<(HeaderHash, SensemakerEntry)>,
    /// index (if any) of our current choice from `next_application_candidates`.
    /// invariant for value `Some(i)`, `0 <= i < next_application_candidates.len()`
    candidate_choice_index: Option<usize>,
}

impl ValidExprState {
    /// compute the `Scheme` which results from applying `expr_sc` to the
    /// `Scheme`s of all `args`.
    fn computed_application_sc(&self) -> Result<Scheme, String> {
        let mut is = InferState::new();

        let Scheme(_, normalized_expr_ty) = normalize(&mut is, self.expr_sc.clone());

        let applicator =
            |acc_ty_res: Result<Type, String>, arg_sc: Scheme| -> Result<Type, String> {
                match acc_ty_res? {
                    Type::TArr(fn_arg_ty, fn_ret_ty) => {
                        // check unification of normalized type
                        let Scheme(_, normalized_arg_ty) = normalize(&mut is, arg_sc);
                        match unifies(normalized_arg_ty, *fn_arg_ty) {
                            Err(msg) => Err(format!("unification error: {:?}", msg)),
                            Ok(_) => Ok(*fn_ret_ty),
                        }
                    }
                    _ => Err("arity mismatch".to_string()),
                }
            };
        let full_application: Type = self
            .args
            .iter()
            .map(|(_, se)| se.output_scheme.clone())
            .fold(Ok(normalized_expr_ty), applicator)?;
        Ok(close_over(full_application))
    }
}

fn ppr_ves(ves: &ValidExprState) -> RcDoc<()> {
    let docs = vec![
        RcDoc::text("scheme:\n"),
        ves.expr_sc.ppr().nest(1).group(),
        RcDoc::text("\nexpr:\n"),
        ppr_expr(&ves.expr).nest(1).group(),
        RcDoc::text("\nargs:\n"),
        RcDoc::text(format!("{:?}", ves.args)).nest(1).group(),
    ];
    RcDoc::concat(docs)
}

impl ExprState {
    fn is_valid(&self) -> bool {
        matches!(self, ExprState::Valid(_))
    }

    fn has_valid_candidate_idx(&self) -> bool {
        match &self {
            ExprState::Valid(ves) => ves.candidate_choice_index.is_some(),
            _ => false,
        }
    }

    fn has_args(&self) -> bool {
        match &self {
            ExprState::Valid(ves) => !ves.args.is_empty(),
            _ => false,
        }
    }
}

enum ViewState {
    Viewer(Vec<SensemakerEntry>),
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

#[derive(Clone)]
pub struct HcInfo {
    pub admin_ws: AdminWebsocket,
    pub app_ws: AppWebsocket,
    pub agent_pk: AgentPubKey,
    pub dna_hash: DnaHash,
}

impl HcInfo {
    async fn get_sensemaker_entries_which_unify(
        &mut self,
        opt_target_sc: Option<Scheme>,
    ) -> Vec<(HeaderHash, SensemakerEntry)> {
        let payload = ExternIO::encode(opt_target_sc).unwrap();
        let cell_id = CellId::new(self.dna_hash.clone(), self.agent_pk.clone());
        let zc = ZomeCall {
            cell_id,
            zome_name: "interpreter".into(),
            fn_name: "get_sensemaker_entries_which_unify".into(),
            payload,
            cap_secret: None,
            provenance: self.agent_pk.clone(),
        };
        let result = self.app_ws.zome_call(zc).await.unwrap();
        result.decode().unwrap()
    }

    async fn create_sensemaker_entry(&mut self, input: CreateSensemakerEntryInput) -> HeaderHash {
        let payload = ExternIO::encode(input).unwrap();
        let cell_id = CellId::new(self.dna_hash.clone(), self.agent_pk.clone());
        let zc = ZomeCall {
            cell_id,
            zome_name: "interpreter".into(),
            fn_name: "create_sensemaker_entry".into(),
            payload,
            cap_secret: None,
            provenance: self.agent_pk.clone(),
        };
        let result = self.app_ws.zome_call(zc).await.unwrap();
        result.decode().unwrap()
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
    opt_events: Option<Events<HcInfo>>,
    event_sender: Sender<Event<HcInfo>>,
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

    async fn get_selection_candidates(&mut self) {
        // zome call to look for SEs which unify with the argument
        if let ExprState::Valid(ves) = &self.expr_state {
            if let Ok(Scheme(tvs, Type::TArr(arg, _))) = ves.computed_application_sc() {
                let opt_target_sc = Some(Scheme(tvs.clone(), *arg.clone()));
                let hash_se_s = self
                    .hc_info
                    .as_mut()
                    .unwrap()
                    .get_sensemaker_entries_which_unify(opt_target_sc)
                    .await;
                self.event_sender
                    .send(Event::SelectorSes(hash_se_s))
                    .expect("send to succeed");
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn error::Error>> {
    // cli arg parsing
    let args = Cli::from_args();

    // terminal initialization
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    {
        let app_ws = AppWebsocket::connect(format!("ws://127.0.0.1:{}", args.hc_app_port))
            .await
            .expect("connect to succeed");
        let mut admin_ws =
            AdminWebsocket::connect(format!("ws://127.0.0.1:{}", args.hc_admin_port))
                .await
                .expect("connect to succeed");
        let agent_pk = admin_ws.generate_agent_pub_key().await.unwrap();
        let dna_hash = {
            let path = Path::new("./happs/social_sensemaker/social_sensemaker.dna");
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

        let pathbuf = PathBuf::from("./happs/social_sensemaker/social_sensemaker.happ");
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
                            Constraint::Length(2),
                            Constraint::Min(15),
                            Constraint::Min(15),
                            Constraint::Min(25),
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
                let mut select_commands = vec![
                    Span::raw(", "),
                    Span::styled("s", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(" to select candidate arg"),
                ];
                let mut deselect_commands = vec![
                    Span::raw(", "),
                    Span::styled("d", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(" to deselect last arg"),
                ];
                let msg = {
                    if app.expr_state.is_valid() {
                        default_commands.append(&mut valid_expr_commands);
                    }
                    if app.expr_state.has_valid_candidate_idx() {
                        default_commands.append(&mut select_commands);
                    }
                    if app.expr_state.has_args() {
                        default_commands.append(&mut deselect_commands);
                    }
                    default_commands.push(Span::raw("."));
                    default_commands
                };

                let help_message =
                    Paragraph::new(Text::from(Spans::from(msg))).wrap(Wrap { trim: false });
                f.render_widget(help_message, chunks[0]);

                let expr_input = Paragraph::new(app.expr_input.as_ref())
                    .style(Style::default())
                    .block(Block::default().borders(Borders::ALL).title("expr input"));
                f.render_widget(expr_input, chunks[1]);

                {
                    let dims = chunks[2];
                    let expr_state_text = match &app.expr_state {
                        ExprState::Invalid(msg) => format!("invalid state:\n\n{}", msg),
                        ExprState::Valid(ves) => {
                            let app_sc_ppr = ves
                                .computed_application_sc()
                                .map(|app_sc| to_pretty(app_sc.ppr(), dims.width.into()));
                            let ppr_string = to_pretty(ppr_ves(ves), dims.width.into());
                            // TODO figure out how to prettyprint this properly
                            format!(
                                "computed_application_sc:\n{:?}\n\n{}",
                                app_sc_ppr, ppr_string
                            )
                        }
                    };
                    let expr_state_msg = Paragraph::new(expr_state_text)
                        .wrap(Wrap { trim: false })
                        .style(Style::default())
                        .block(Block::default().borders(Borders::ALL).title("expr_state"));
                    f.render_widget(expr_state_msg, dims);
                }

                {
                    let dims = chunks[3];
                    let mut text: Text = Text::from("");
                    if let ExprState::Valid(ves) = &app.expr_state {
                        let candidate_vec: Vec<Spans> = ves
                            .next_application_candidates
                            .iter()
                            .enumerate()
                            .map(|(idx, candidate)| {
                                let mut style = Style::default();
                                if Some(idx) == ves.candidate_choice_index {
                                    style = style.add_modifier(Modifier::BOLD);
                                }
                                Spans::from(Span::styled(format!("{:?}", candidate), style))
                            })
                            .collect();
                        text.extend(candidate_vec);
                    };
                    let msg = Paragraph::new(text)
                        .wrap(Wrap { trim: false })
                        .style(Style::default())
                        .block(Block::default().borders(Borders::ALL).title("SE selector"));
                    f.render_widget(msg, dims);
                }

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
                f.render_widget(app_info, chunks[4]);
            }

            ViewState::Viewer(se_s) => {
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

                let rendered_se_s: String = se_s
                    .iter()
                    .map(|se| to_pretty(se.ppr(), chunks[1].width.into()))
                    .collect::<Vec<String>>()
                    .join("\n\n");

                let block = Paragraph::new(rendered_se_s)
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
                app.expr_state = match expr().easy_parse(position::Stream::new(&app.expr_input[..]))
                {
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
                                Ok(expr_sc) => ExprState::Valid(ValidExprState {
                                    expr_sc,
                                    expr,
                                    args: vec![],
                                    next_application_candidates: vec![],
                                    candidate_choice_index: None,
                                }),
                            }
                        }
                    }
                };
                app.get_selection_candidates().await;
            }
            Event::Input(Key::Char('c')) if app.view_state.is_creator() => {
                match (&app.expr_state, &mut app.hc_info) {
                    (ExprState::Invalid(_), _) => {} // invalid expr
                    (_, None) => {}                  // no hc_ws client
                    (ExprState::Valid(ves), Some(hc_info)) => {
                        let args = ves
                            .args
                            .iter()
                            .map(|(e_hash, _se)| {
                                SensemakerOperand::SensemakerOperand(e_hash.clone())
                            })
                            .collect();
                        let input = CreateSensemakerEntryInput {
                            expr: ves.expr.clone(),
                            args,
                        };
                        let se_hash = hc_info.create_sensemaker_entry(input).await;
                        app.log_hc_response(format!("create: se_hash: {:?}", se_hash));
                    }
                }
            }
            Event::Input(Key::Down) if app.view_state.is_creator() => {
                if let ExprState::Valid(ves) = &mut app.expr_state {
                    ves.candidate_choice_index = ves
                        .candidate_choice_index
                        // this allows us to do a max comparison with "1 less than the len"
                        // without fear of underflow on an usize.
                        .map(|i| cmp::min(i + 2, ves.next_application_candidates.len()) - 1);
                }
            }
            Event::Input(Key::Up) if app.view_state.is_creator() => {
                if let ExprState::Valid(ves) = &mut app.expr_state {
                    ves.candidate_choice_index =
                        ves.candidate_choice_index.map(|i| i.saturating_sub(1));
                }
            }
            Event::Input(Key::Char('s')) if app.expr_state.has_valid_candidate_idx() => {
                match &mut app.expr_state {
                    ExprState::Invalid(_) => {} // should be unreachable due to match guard
                    ExprState::Valid(ves) => {
                        // should be safe due to match guard
                        let idx = ves.candidate_choice_index.unwrap();
                        // TODO is it possible to avoid cloning and pull the memory out of the Vec?
                        let selection = ves.next_application_candidates[idx].clone();
                        ves.next_application_candidates = vec![];
                        ves.args.push(selection);
                        ves.candidate_choice_index = None;
                    }
                }
                app.get_selection_candidates().await;
            }
            Event::Input(Key::Char('d')) if app.expr_state.has_args() => {
                match &mut app.expr_state {
                    ExprState::Invalid(_) => {} // should be unreachable due to match guard
                    ExprState::Valid(ves) => {
                        ves.next_application_candidates = vec![];
                        ves.args.pop();
                        ves.candidate_choice_index = None;
                    }
                }
                app.get_selection_candidates().await;
            }
            Event::Input(Key::Char('\t')) => {
                app.view_state = app.view_state.toggle();
                // TODO this cloning could maybe be eliminated
                let mut hc_info = app.hc_info.clone().unwrap();

                if app.view_state.is_viewer() {
                    let opt_target_sc: Option<Scheme> = None;
                    let hash_se_s: Vec<(HeaderHash, SensemakerEntry)> = hc_info
                        .get_sensemaker_entries_which_unify(opt_target_sc)
                        .await;
                    let se_s = hash_se_s.into_iter().map(|(_eh, se)| se).collect();
                    app.event_sender
                        .send(Event::ViewerSes(se_s))
                        .expect("send to succeed");
                }
            }
            Event::HcInfo(hc_info) => {
                app.hc_info = Some(hc_info);
                app.log_hc_response("hc_info: connected".into());
            }
            Event::SelectorSes(hash_se_s) => {
                app.log_hc_response("got selector SEs".into());
                if let ExprState::Valid(ves) = &mut app.expr_state {
                    if !hash_se_s.is_empty() {
                        ves.candidate_choice_index = Some(0);
                    }
                    ves.next_application_candidates = hash_se_s;
                }
            }
            Event::ViewerSes(se_s) => {
                app.log_hc_response("got viewer SEs".into());
                app.view_state = ViewState::Viewer(se_s);
            }
            _ => {}
        }
    }
    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
// CLI arg parsing
////////////////////////////////////////////////////////////////////////////////

#[derive(StructOpt, Debug)]
#[structopt(about = HELP)]
struct Cli {
    /// Holochain app port
    #[structopt(long, short = "p", default_value = "9999")]
    hc_app_port: u64,

    /// Holochain admin port
    #[structopt(long, short = "f", default_value = "9000")]
    hc_admin_port: u64,
}

const HELP: &str = r#"
############################
#                          #
# ██████╗░██╗░░░░░██████╗░ #
# ██╔══██╗██║░░░░░██╔══██╗ #
# ██████╔╝██║░░░░░██████╔╝ #
# ██╔══██╗██║░░░░░██╔═══╝░ #
# ██║░░██║███████╗██║░░░░░ #
# ╚═╝░░╚═╝╚══════╝╚═╝░░░░░ #
#                          #
############################
"#;
