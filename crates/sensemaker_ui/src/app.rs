use wasm_bindgen::prelude::*;
use weblog::{console_error, console_log};
use yew::prelude::*;

use holochain_client_wrapper::{
    AdminWebsocket, AdminWsCmd, AdminWsCmdResponse, AppWebsocket, AppWsCmd, AppWsCmdResponse,
    CellId, DeserializeFromJsObj, EntryHashRaw, EntryHeaderHashPairRaw,
};

pub enum Msg {
    AdminWs(WsMsg<AdminWsCmd, AdminWsCmdResponse>),
    AppWs(WsMsg<AppWsCmd, AppWsCmdResponse>),
    Log(String),
    Error(String),
}

pub enum WsMsg<WSCMD, WSCMDRESP> {
    Cmd(WSCMD),
    CmdResponse(Result<WSCMDRESP, JsValue>),
}

pub struct Model {
    admin_ws: AdminWebsocket,
    app_ws: AppWebsocket,
}

#[derive(Properties, PartialEq)]
pub struct ModelProps {
    pub admin_ws_js: JsValue,
    pub app_ws_js: JsValue,
    pub cell_id_js: JsValue,
}

impl Component for Model {
    type Message = Msg;
    type Properties = ModelProps;

    fn create(ctx: &Context<Self>) -> Self {
        let props = ctx.props();
        let cell_id = CellId::deserialize_from_js_obj(props.cell_id_js.clone());
        let cell_id_ = cell_id.clone();
        let app_ws: AppWebsocket = props.app_ws_js.clone().into();
        let app_ws_: AppWebsocket = app_ws.clone();
        let admin_ws: AdminWebsocket = props.admin_ws_js.clone().into();
        let admin_ws_ = admin_ws.clone();
        ctx.link().send_future(async move {
            let resp = admin_ws_.call(AdminWsCmd::ListActiveApps).await;
            match resp {
                Ok(AdminWsCmdResponse::ListActiveApps(active_apps)) => {
                    Msg::Log("TODO test-presence / install memez+paperz here".into())
                }
                Ok(resp) => Msg::Error(format!("impossible: invalid response: {:?}", resp)),
                Err(err) => Msg::Error(format!("err: {:?}", err)),
            }
        });
        Self { admin_ws, app_ws }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::AdminWs(ws_msg) => match ws_msg {
                WsMsg::Cmd(cmd) => {
                    let ws = self.admin_ws.clone();
                    ctx.link().send_future(async move {
                        Msg::AdminWs(WsMsg::CmdResponse(ws.call(cmd).await))
                    });
                    false
                }

                WsMsg::CmdResponse(resp) => {
                    match resp {
                        Ok(val) => {
                            console_log!(format!("WsMsg::CmdResponse: {:?}", val));
                        }
                        Err(err) => {
                            console_error!(format!("WsMsg::CmdResponse: error: {:?}", err));
                        }
                    };
                    false
                }
            },

            Msg::AppWs(ws_msg) => match ws_msg {
                WsMsg::Cmd(cmd) => {
                    let ws = self.app_ws.clone();
                    ctx.link().send_future(async move {
                        Msg::AppWs(WsMsg::CmdResponse(ws.call(cmd).await))
                    });
                    false
                }

                WsMsg::CmdResponse(resp) => {
                    match resp {
                        Ok(val) => {
                            console_log!(format!("WsMsg::CmdResponse: {:?}", val));
                        }
                        Err(err) => {
                            console_error!(format!("WsMsg::CmdResponse: error: {:?}", err));
                        }
                    };
                    false
                }
            },

            Msg::Error(err) => {
                console_error!("Error: ", err);
                false
            }

            Msg::Log(err) => {
                console_log!("Log: ", err);
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div>
                <p>{"hello, sensemaker 👋"}</p>
            </div>
        }
    }
}
