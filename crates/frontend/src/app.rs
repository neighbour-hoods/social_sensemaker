use combine::{stream::position, EasyParser, StreamOnce};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use reqwasm::websocket::{Message, WebSocket, WebSocketError};
use weblog::{console_error, console_log};
use web_sys::HtmlInputElement as InputElement;
use yew::{events::KeyboardEvent, html, html::Scope, prelude::*};

use rep_lang_concrete_syntax::parse::expr;
use rep_lang_core::abstract_syntax::Expr;
use rep_lang_runtime::{env::Env, infer::infer_expr, types::Scheme};

#[derive(Debug)]
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

pub enum HcClient {
    Present(WsPair),
    Absent(String),
}

type WsPair = (UnboundedSender<Message>, UnboundedReceiver<Result<Message, WebSocketError>>);

#[allow(dead_code)]
pub enum Msg {
    ExprEdit(String),
    HcClientConnected(WsPair),
    HcClientError(String),
    CreateExpr,
}

pub struct Model {
    expr_state: ExprState,
    hc_client: HcClient,
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        ctx.link().send_future(async {
            match WebSocket::open("ws://127.0.0.1:9999") {
                Ok(ws) => Msg::HcClientConnected((ws.sender, ws.receiver)),
                Err(err) => Msg::HcClientError(format!("reqwasm WebSocket::open failed : {}", err)),
            }
        });

        Self {
            expr_state: ExprState::Invalid("init".into()),
            hc_client: HcClient::Absent("".into()),
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::ExprEdit(text) => {
                let st = match expr().easy_parse(position::Stream::new(&text[..])) {
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
                self.expr_state = st;
            }
            Msg::HcClientConnected(t) => {
                self.hc_client = HcClient::Present(t);
            }
            Msg::HcClientError(err) => {
                self.hc_client = HcClient::Absent(err.clone());
                console_error!(err)
            }
            Msg::CreateExpr => {
                console_log!(format!("create expr @ {:?}", self.expr_state));
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div class="rlp-wrapper">
                <div id="expr_input">
                    <h1>{ "expr" }</h1>
                    { self.view_input(ctx.link()) }
                </div>
                <div id="expr_msgs">
                    <h1>{ "msgs" }</h1>
                    { self.view_msgs() }
                </div>
                { self.hc_client_status() }
            </div>
        }
    }
}

impl Model {
    fn view_input(&self, link: &Scope<Self>) -> Html {
        let onkeypress = link.batch_callback(|e: KeyboardEvent| {
            let input: InputElement = e.target_unchecked_into();
            Some(Msg::ExprEdit(input.value()))
        });
        let onclick = link.callback(|_| Msg::CreateExpr);
        let disabled = !self.expr_state.is_valid();
        html! {
            <div id="expr-input">
                <textarea
                    class="new-expr"
                    placeholder="(lam [x] x)"
                    {onkeypress}
                />
                <button onclick={onclick} disabled={disabled}>{ "create" }</button>
            </div>
        }
    }

    fn view_msgs(&self) -> Html {
        match &self.expr_state {
            ExprState::Valid(sc, expr) => html! {
                <p>{format!("Valid: type: {:?}, expr: {:?}", sc, expr)}</p>
            },
            ExprState::Invalid(msg) => html! {
                <p>{format!("Invalid: {}", msg)}</p>
            },
        }
    }

    fn hc_client_status(&self) -> Html {
        let (color, text) = match &self.hc_client {
            HcClient::Present(_) => ("green", "present".into()),
            HcClient::Absent(err) => ("red", format!("absent {}", err)),
        };
        html! {
            <div id="hc_client_status">
                <h1>{ "Holochain Client Status: " }</h1>
                <font color={ color }>
                    { text }
                </font>
            </div>
        }
    }
}
