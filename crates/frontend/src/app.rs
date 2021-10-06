use combine::{stream::position, EasyParser, StreamOnce};
use web_sys::HtmlInputElement as InputElement;
use yew::{events::KeyboardEvent, html, html::Scope, prelude::*};

use rep_lang_concrete_syntax::parse::expr;
use rep_lang_core::abstract_syntax::Expr;
use rep_lang_runtime::{env::Env, infer::infer_expr, types::Scheme};

pub enum ExprState {
    Valid(Scheme, Expr),
    Invalid(String),
}

pub enum Msg {
    ExprEdit(String),
}

pub struct Model {
    expr_state: ExprState,
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            expr_state: ExprState::Invalid("init".to_string()),
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
                    { self.view_msgs(ctx.link()) }
                </div>
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

    fn view_msgs(&self, _link: &Scope<Self>) -> Html {
        match &self.expr_state {
            ExprState::Valid(sc, expr) => html! {
                <p>{format!("Valid: type: {:?}, expr: {:?}", sc, expr)}</p>
            },
            ExprState::Invalid(msg) => html! {
                <p>{format!("Invalid: {}", msg)}</p>
            },
        }
    }
}
