use combine::{stream::position, EasyParser, StreamOnce};
use jsonrpc_core_client::TypedClient;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement as InputElement;
use yew::{events::KeyboardEvent, html, html::Scope, prelude::*};

use rep_lang_concrete_syntax::parse::expr;
use rep_lang_core::abstract_syntax::Expr;
use rep_lang_runtime::{env::Env, infer::infer_expr, types::Scheme};

pub enum ExprState {
    Valid(Scheme, Expr),
    Invalid(String),
}

pub enum HcClient {
    Present(TypedClient),
    Absent,
}

#[allow(dead_code)]
pub enum Msg {
    ExprEdit(String),
    HcClientConnected(TypedClient),
}

pub struct Model {
    expr_state: ExprState,
    hc_client: HcClient,
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        spawn_local(async move {
            match ws::try_connect("127.0.0.1:8888") {
                Ok(tc) => ctx.link().send_self(Msg::HcClientConnected(tc)),
                Err(err) => { } // TODO: send error msg?
            };
        });

        Self {
            expr_state: ExprState::Invalid("init".to_string()),
            hc_client: HcClient::Absent,
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
            Msg::HcClientConnected(tc) => {
                self.hc_client = HcClient::Present(tc);
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
        html! {
            <textarea
                class="new-expr"
                placeholder="(lam [x] x)"
                {onkeypress}
            />
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
            HcClient::Present(_) => ("green", "present"),
            HcClient::Absent => ("red", "absent"),
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
