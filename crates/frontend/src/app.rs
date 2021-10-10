use combine::{stream::position, EasyParser, StreamOnce};
use futures::{channel::mpsc::{UnboundedReceiver, UnboundedSender}, SinkExt};
use holo_hash::HoloHash;
use holochain_serialized_bytes;
use holochain_zome_types::{call::Call, zome_io::ExternIO};
use reqwasm::websocket::{Message, WebSocket, WebSocketError};
use std::iter;
use weblog::{console_error, console_log};
use web_sys::HtmlInputElement as InputElement;
use yew::{events::KeyboardEvent, html, html::Scope, prelude::*};

use common::CreateInterchangeEntryInput;
use rep_lang_concrete_syntax::parse::expr;
use rep_lang_core::abstract_syntax::Expr;
use rep_lang_runtime::{env::Env, infer::infer_expr, types::Scheme};

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

pub enum HcClient {
    Present(WsPair),
    Absent(String),
}

impl HcClient {
    fn is_present(&self) -> bool {
        match self {
            HcClient::Present(_) => true,
            HcClient::Absent(_) => false,
        }
    }
}

type WsPair = (UnboundedSender<Message>, UnboundedReceiver<Result<Message, WebSocketError>>);

#[allow(dead_code)]
pub enum Msg {
    ExprEdit(String),
    HcClientConnected(WsPair),
    HcClientError(String),
    CreateExpr,
    CreateExprResponse(String),
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

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
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
                match (&self.hc_client, self.expr_state.clone()) {
                    (HcClient::Absent(_), _) => {
                        console_error!("attempted to create expr with absent HcClient");
                    }
                    (_, ExprState::Invalid(_)) => {
                        console_error!("attempted to create expr with invalid ExprState");
                    }
                    (HcClient::Present((send, _recv)), ExprState::Valid(_sc, expr)) => {
                        let mut send2 = send.clone();
                        ctx.link().send_future(async move {
                            let input: CreateInterchangeEntryInput = CreateInterchangeEntryInput {
                                expr,
                                args: Vec::new(),
                            };
                            let input_bytes = ExternIO::encode(input).unwrap();
                            let agent_pk_bytes: Vec<u8> = iter::repeat(1).take(36).collect();
                            let agent_pk = HoloHash::from_raw_36(agent_pk_bytes);
                            let call = Call::new(None, "interpreter".into(), "create_interchange_entry".into(), None, input_bytes, agent_pk);
                            let msg: Message = Message::Bytes(holochain_serialized_bytes::encode(&call).unwrap());
                            send2.send(msg).await.unwrap();
                            Msg::CreateExprResponse("sent".into())
                            // match recv.next().await {
                            //     Some(Ok(Message::Text(m))) => Msg::CreateExprResponse(format!("text: {}", m)),
                            //     Some(Ok(Message::Bytes(m))) => Msg::CreateExprResponse(format!("bytes: {:?}", m)),
                            //     Some(Err(e)) => Msg::CreateExprResponse(format!("error: {}", e)),
                            //     None => Msg::CreateExprResponse("None".into()),
                            // }
                        });
                    }
                }
            }
            Msg::CreateExprResponse(msg) => {
                console_log!(msg);
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
        let disabled = !self.expr_state.is_valid() || !self.hc_client.is_present();
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
