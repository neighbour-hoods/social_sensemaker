use wasm_bindgen::prelude::*;
use weblog::{console_error, console_log};
use yew::prelude::*;

use holochain_client_wrapper::{
    AdminWebsocket, AdminWsCmd, AdminWsCmdResponse, AppWebsocket, AppWsCmd, AppWsCmdResponse,
    CellId, DeserializeFromJsObj, HashRoleProof,
};

pub enum Msg {
    AdminWs(WsMsg<AdminWsCmd, AdminWsCmdResponse>),
    AppWs(WsMsg<AppWsCmd, AppWsCmdResponse>),
    Log(String),
    Error(String),
    WidgetsInstalled,
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
        let admin_ws: AdminWebsocket = props.admin_ws_js.clone().into();
        let admin_ws_ = admin_ws.clone();
        ctx.link().send_future(async move {
            let ret = async {
                let active_apps = match admin_ws_.call(AdminWsCmd::ListActiveApps).await {
                    Ok(AdminWsCmdResponse::ListActiveApps(x)) => Ok(x),
                    Ok(resp) => Err(format!(
                        "impossible: invalid response
{:?}",
                        resp
                    )),
                    Err(err) => Err(format!("err: {:?}", err)),
                }?;

                let target_dna_pairs: Vec<(String, String)> = vec![
                    ("memez_main_zome", "../widgets_rs/happs/memez/memez.dna"),
                    ("paperz_main_zome", "../widgets_rs/happs/paperz/paperz.dna"),
                ]
                .into_iter()
                .map(|(x, y)| (x.into(), y.into()))
                .collect();

                let mut all_succeeded = true;
                for (target_dna_pair_name, target_dna_pair_path) in target_dna_pairs {
                    // TODO we could handle the cases of installed-but-not-enabled, etc, later.
                    if active_apps.contains(&target_dna_pair_name) {
                        console_log!(format!("dna {} is already active", target_dna_pair_name));
                    } else {
                        // TODO this error handling is a bit brittle
                        match install_enable_dna(
                            cell_id_.clone(),
                            admin_ws_.clone(),
                            target_dna_pair_name,
                            target_dna_pair_path,
                        )
                        .await
                        {
                            Ok(()) => {}
                            Err(err) => {
                                console_error!(err);
                                all_succeeded = false;
                            }
                        }
                    }
                }
                Ok(all_succeeded)
            };
            match ret.await {
                Err(err) => Msg::Error(err),
                Ok(false) => Msg::Error("see console error log".into()),
                Ok(true) => Msg::WidgetsInstalled,
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

            Msg::WidgetsInstalled => {
                console_log!("widgets installed!");
                false
            }
        }
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        html! {
            <div>
                <p>{"hello, sensemaker 👋"}</p>
            </div>
        }
    }
}
async fn install_enable_dna(
    cell_id: CellId,
    ws: AdminWebsocket,
    installed_app_id: String,
    path: String,
) -> Result<(), String> {
    let cmd = AdminWsCmd::RegisterDna {
        path,
        uid: None,
        properties: None,
    };
    let dna_hash = match ws.call(cmd).await {
        Ok(AdminWsCmdResponse::RegisterDna(x)) => Ok(x),
        Ok(resp) => Err(format!("impossible: invalid response: {:?}", resp)),
        Err(err) => Err(format!("err: {:?}", err)),
    }?;
    let cmd = AdminWsCmd::InstallApp {
        installed_app_id: installed_app_id.clone(),
        agent_key: cell_id.1,
        dnas: vec![HashRoleProof {
            hash: dna_hash,
            role_id: "thedna".into(),
            membrane_proof: None,
        }],
    };
    let install_app = match ws.call(cmd).await {
        Ok(AdminWsCmdResponse::InstallApp(x)) => Ok(x),
        Ok(resp) => Err(format!("impossible: invalid response: {:?}", resp)),
        Err(err) => Err(format!("err: {:?}", err)),
    }?;
    console_log!(format!("install_app: {:?}", install_app));
    let cmd = AdminWsCmd::EnableApp { installed_app_id };
    let enable_app = match ws.call(cmd).await {
        Ok(AdminWsCmdResponse::EnableApp(x)) => Ok(x),
        Ok(resp) => Err(format!("impossible: invalid response: {:?}", resp)),
        Err(err) => Err(format!("err: {:?}", err)),
    }?;
    console_log!(format!("enable_app: {:?}", enable_app));
    Ok(())
}
