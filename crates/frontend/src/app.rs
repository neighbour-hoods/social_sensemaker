use combine::{stream::position, EasyParser, StreamOnce};
use futures::{
    channel::mpsc::{UnboundedReceiver, UnboundedSender},
    SinkExt,
};
use holo_hash::HoloHash;
use holochain_serialized_bytes;
use holochain_serialized_bytes::holochain_serial;
use holochain_serialized_bytes_derive::SerializedBytes;
use holochain_zome_types::{call::Call, zome_io::ExternIO};
use reqwasm::websocket::{Message, WebSocket, WebSocketError};
use serde;
use std::iter;
use web_sys::HtmlInputElement as InputElement;
use weblog::{console_error, console_log};
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

type WsPair = (
    UnboundedSender<Message>,
    UnboundedReceiver<Result<Message, WebSocketError>>,
);

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
    request_id: u64,
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
            request_id: 0,
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
                        let req_id = self.next_request_id();
                        ctx.link().send_future(async move {
                            let input: CreateInterchangeEntryInput = CreateInterchangeEntryInput {
                                expr,
                                args: Vec::new(),
                            };
                            let input_bytes = ExternIO::encode(input).unwrap();
                            let input2: CreateInterchangeEntryInput =
                                ExternIO::decode(&input_bytes).unwrap();
                            console_log!(format!("{:?}", input2));
                            let agent_pk_bytes: Vec<u8> = iter::repeat(1).take(36).collect();
                            let agent_pk = HoloHash::from_raw_36(agent_pk_bytes);
                            let call = Call::new(
                                None,
                                "interpreter".into(),
                                "create_interchange_entry".into(),
                                None,
                                input_bytes,
                                agent_pk,
                            );
                            let msg_bytes = holochain_serialized_bytes::encode(&call).unwrap();
                            let call2: Call =
                                holochain_serialized_bytes::decode(&msg_bytes).unwrap();
                            console_log!(format!("{:?}", call2));
                            let req = WireMessage::Request {
                                id: req_id,
                                data: msg_bytes,
                            };
                            let req_bytes = holochain_serialized_bytes::encode(&req).unwrap();
                            send2.send(Message::Bytes(req_bytes)).await.unwrap();
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

    fn next_request_id(&mut self) -> u64 {
        let i = self.request_id;
        self.request_id += 1;
        i
    }
}

// taken from https://github.com/holochain/holochain/blob/8cade151329117c40e47533449a2f842187c373a/crates/holochain_websocket/src/lib.rs#L138-L167
// we are unable to use it directly because `holochain_websocket` depends on `net2` and cannot
// build on wasm.
#[derive(Debug, serde::Serialize, serde::Deserialize, SerializedBytes)]
#[serde(tag = "type")]
/// The messages actually sent over the wire by this library.
/// If you want to impliment your own server or client you
/// will need this type or be able to serialize / deserialize it.
pub enum WireMessage {
    /// A message without a response.
    Signal {
        #[serde(with = "serde_bytes")]
        /// Actual bytes of the message serialized as [message pack](https://msgpack.org/).
        data: Vec<u8>,
    },
    /// A request that requires a response.
    Request {
        /// The id of this request.
        /// Note ids are recycled once they are used.
        id: u64,
        #[serde(with = "serde_bytes")]
        /// Actual bytes of the message serialized as [message pack](https://msgpack.org/).
        data: Vec<u8>,
    },
    /// The response to a request.
    Response {
        /// The id of the request that this response is for.
        id: u64,
        #[serde(with = "serde_bytes")]
        /// Actual bytes of the message serialized as [message pack](https://msgpack.org/).
        data: Option<Vec<u8>>,
    },
}
