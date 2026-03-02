use gloo_timers::callback::Interval;
use leptos::*;
use leptos_meta::*;
use leptos_router::*;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

mod api;
mod wallet;
mod wallet_crypto;

/// Payment requirements from 402 response
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaymentRequirements {
    pub scheme: String,
    pub network: String,
    pub price: String,
    pub asset: String,
    pub amount: String,
    #[serde(rename = "payTo")]
    pub pay_to: String,
    #[serde(rename = "maxTimeoutSeconds")]
    pub max_timeout_seconds: u64,
    pub description: Option<String>,
}

/// Settlement response from server
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SettleResponse {
    pub success: bool,
    pub transaction: Option<String>,
    pub network: String,
    pub payer: Option<String>,
}

/// Wallet connection mode
#[derive(Clone, Debug, Default, PartialEq)]
pub enum WalletMode {
    #[default]
    Disconnected,
    MetaMask,
    DemoKey,
    Embedded,
}

impl WalletMode {
    fn label(&self) -> &'static str {
        match self {
            WalletMode::Disconnected => "",
            WalletMode::MetaMask => "MetaMask",
            WalletMode::DemoKey => "Demo",
            WalletMode::Embedded => "Embedded",
        }
    }
}

/// App state for wallet connection
#[derive(Clone, Debug, Default)]
pub struct WalletState {
    pub connected: bool,
    pub address: Option<String>,
    pub chain_id: Option<String>,
    pub mode: WalletMode,
    pub private_key: Option<String>,
}

/// Main application component
#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    let (wallet, set_wallet) = create_signal(WalletState::default());
    provide_context((wallet, set_wallet));

    view! {
        <Html lang="en" />
        <Meta charset="utf-8" />
        <Meta name="viewport" content="width=device-width, initial-scale=1" />
        <Title text="x402 Demo - Pay-per-Request APIs" />
        <Stylesheet href="/style.css" />

        <Router>
            <main class="container">
                <Header />
                <Routes>
                    <Route path="/" view=HomePage />
                    <Route path="/dashboard" view=DashboardPage />
                    <Route path="/chat" view=ChatPage />
                    <Route path="/*any" view=NotFound />
                </Routes>
                <Footer />
            </main>
        </Router>
    }
}

/// Header with navigation and wallet connection
#[component]
fn Header() -> impl IntoView {
    let (wallet, set_wallet) =
        expect_context::<(ReadSignal<WalletState>, WriteSignal<WalletState>)>();

    let location = use_location();
    let (mobile_open, set_mobile_open) = create_signal(false);

    let toggle_mobile = move |_| set_mobile_open.update(|v| *v = !*v);

    view! {
        <header class="header">
            <nav class="nav">
                <a href="/" class="logo">"x402"</a>
                <button class="mobile-nav-toggle" on:click=toggle_mobile>
                    {move || if mobile_open.get() { "\u{2715}" } else { "\u{2630}" }}
                </button>
                <div class=move || {
                    if mobile_open.get() { "nav-links open" } else { "nav-links" }
                }>
                    {move || {
                        let path = location.pathname.get();
                        view! {
                            <a
                                href="/"
                                class=if path == "/" { "active" } else { "" }
                                on:click=move |_| set_mobile_open.set(false)
                            >"Demo"</a>
                            <a
                                href="/dashboard"
                                class=if path == "/dashboard" { "active" } else { "" }
                                on:click=move |_| set_mobile_open.set(false)
                            >"Dashboard"</a>
                            <a
                                href="/chat"
                                class=if path == "/chat" { "active" } else { "" }
                                on:click=move |_| set_mobile_open.set(false)
                            >"Chat"</a>
                            <a href="https://docs.rs/tempo-x402" target="_blank">"Docs"</a>
                            <a href="https://github.com/compusophy/tempo-x402" target="_blank">"GitHub"</a>
                        }
                    }}
                </div>
                <WalletButtons wallet=wallet set_wallet=set_wallet />
            </nav>
        </header>
    }
}

/// Wallet connect/disconnect buttons with three modes
#[component]
fn WalletButtons(
    wallet: ReadSignal<WalletState>,
    set_wallet: WriteSignal<WalletState>,
) -> impl IntoView {
    let (funding, set_funding) = create_signal(false);

    let connect_metamask = move |_| {
        spawn_local(async move {
            match wallet::connect_wallet().await {
                Ok(state) => set_wallet.set(state),
                Err(e) => {
                    web_sys::console::error_1(&format!("MetaMask error: {}", e).into());
                }
            }
        });
    };

    let use_demo = move |_| match wallet::use_demo_key() {
        Ok(state) => set_wallet.set(state),
        Err(e) => {
            web_sys::console::error_1(&format!("Demo key error: {}", e).into());
        }
    };

    let create_wallet = move |_| {
        set_funding.set(true);
        match wallet::load_or_create_embedded_wallet() {
            Ok((state, is_new)) => {
                let address = state.address.clone().unwrap_or_default();
                set_wallet.set(state);
                if is_new {
                    spawn_local(async move {
                        match wallet::fund_address(&address).await {
                            Ok(_) => {
                                web_sys::console::log_1(
                                    &format!("Funded new wallet: {}", address).into(),
                                );
                            }
                            Err(e) => {
                                web_sys::console::error_1(&format!("Funding failed: {}", e).into());
                            }
                        }
                        set_funding.set(false);
                    });
                } else {
                    web_sys::console::log_1(&format!("Restored wallet: {}", address).into());
                    set_funding.set(false);
                }
            }
            Err(e) => {
                web_sys::console::error_1(&format!("Create wallet error: {}", e).into());
                set_funding.set(false);
            }
        }
    };

    let disconnect = move |_| {
        set_wallet.set(WalletState::default());
    };

    view! {
        <Show
            when=move || wallet.get().connected
            fallback=move || view! {
                <div class="wallet-buttons">
                    <button class="btn btn-primary" on:click=connect_metamask>
                        "Connect Wallet"
                    </button>
                    <button class="btn btn-secondary" on:click=use_demo>
                        "Demo Key"
                    </button>
                    <button
                        class="btn btn-secondary"
                        on:click=create_wallet
                        disabled=move || funding.get()
                    >
                        {move || {
                            if funding.get() {
                                "Creating..."
                            } else if wallet::has_stored_wallet() {
                                "Restore Wallet"
                            } else {
                                "Create Wallet"
                            }
                        }}
                    </button>
                </div>
            }
        >
            {move || {
                let w = wallet.get();
                let addr = w.address.unwrap_or_default();
                let short = if addr.len() > 10 {
                    format!("{}...{}", &addr[..6], &addr[addr.len()-4..])
                } else {
                    addr
                };
                let mode_label = w.mode.label();
                view! {
                    <div class="wallet-info">
                        <span class="wallet-mode-badge">{mode_label}</span>
                        <span class="wallet-address">{short}</span>
                        <button class="btn btn-secondary btn-sm" on:click=disconnect>
                            "Disconnect"
                        </button>
                    </div>
                }
            }}
        </Show>
    }
}

/// Wallet management panel — export, import, delete for embedded wallets.
#[component]
fn WalletManagement() -> impl IntoView {
    let (wallet, set_wallet) =
        expect_context::<(ReadSignal<WalletState>, WriteSignal<WalletState>)>();

    let (show_key, set_show_key) = create_signal(false);
    let (show_import, set_show_import) = create_signal(false);
    let (import_value, set_import_value) = create_signal(String::new());
    let (import_error, set_import_error) = create_signal(None::<String>);
    let (confirm_delete, set_confirm_delete) = create_signal(false);
    let (copied, set_copied) = create_signal(false);

    let toggle_reveal = move |_| {
        set_show_key.update(|v| *v = !*v);
        set_copied.set(false);
    };

    let copy_key = move |_| {
        if let Some(key) = wallet.get().private_key {
            wallet::copy_to_clipboard(&key);
            set_copied.set(true);
        }
    };

    let download_json = move |_| {
        let w = wallet.get();
        if let (Some(key), Some(addr)) = (w.private_key, w.address) {
            let json = wallet::export_wallet_json(&key, &addr);
            let filename = format!("x402-wallet-{}.json", &addr[..8]);
            if let Err(e) = wallet::trigger_download(&filename, &json) {
                web_sys::console::error_1(&format!("Download failed: {}", e).into());
            }
        }
    };

    let toggle_import = move |_| {
        set_show_import.update(|v| *v = !*v);
        set_import_error.set(None);
        set_import_value.set(String::new());
    };

    let do_import = move |_| {
        let key = import_value.get();
        match wallet::import_embedded_wallet(&key) {
            Ok(state) => {
                set_wallet.set(state);
                set_show_import.set(false);
                set_import_error.set(None);
            }
            Err(e) => {
                set_import_error.set(Some(e));
            }
        }
    };

    let do_delete = move |_| {
        wallet::delete_embedded_wallet();
        set_wallet.set(WalletState::default());
        set_confirm_delete.set(false);
    };

    view! {
        <Show when=move || wallet.get().mode == WalletMode::Embedded fallback=|| ()>
            <div class="wallet-management">
                <h4>"Wallet Management"</h4>

                <div class="wallet-actions">
                    <button class="btn btn-secondary btn-sm" on:click=toggle_reveal>
                        {move || if show_key.get() { "Hide Key" } else { "Reveal Key" }}
                    </button>

                    <Show when=move || show_key.get() fallback=|| ()>
                        <div class="key-reveal">
                            <code class="private-key">{move || wallet.get().private_key.unwrap_or_default()}</code>
                            <button class="btn btn-secondary btn-sm" on:click=copy_key>
                                {move || if copied.get() { "Copied!" } else { "Copy" }}
                            </button>
                        </div>
                    </Show>

                    <button class="btn btn-secondary btn-sm" on:click=download_json>
                        "Download Backup"
                    </button>

                    <button class="btn btn-secondary btn-sm" on:click=toggle_import>
                        {move || if show_import.get() { "Cancel Import" } else { "Import Key" }}
                    </button>

                    <Show when=move || show_import.get() fallback=|| ()>
                        <div class="import-form">
                            <input
                                type="password"
                                class="input"
                                placeholder="Paste private key (0x...)"
                                prop:value=move || import_value.get()
                                on:input=move |ev| {
                                    set_import_value.set(event_target_value(&ev));
                                    set_import_error.set(None);
                                }
                            />
                            <button class="btn btn-primary btn-sm" on:click=do_import>
                                "Import"
                            </button>
                            <Show when=move || import_error.get().is_some() fallback=|| ()>
                                <p class="error-text">{move || import_error.get().unwrap_or_default()}</p>
                            </Show>
                        </div>
                    </Show>

                    <Show
                        when=move || confirm_delete.get()
                        fallback=move || view! {
                            <button
                                class="btn btn-danger btn-sm"
                                on:click=move |_| set_confirm_delete.set(true)
                            >
                                "Delete Wallet"
                            </button>
                        }
                    >
                        <div class="delete-confirm">
                            <p class="warning-text">
                                "This permanently deletes your private key. Make sure you have a backup!"
                            </p>
                            <button class="btn btn-danger btn-sm" on:click=do_delete>
                                "Yes, Delete Forever"
                            </button>
                            <button
                                class="btn btn-secondary btn-sm"
                                on:click=move |_| set_confirm_delete.set(false)
                            >
                                "Cancel"
                            </button>
                        </div>
                    </Show>
                </div>
            </div>
        </Show>
    }
}

/// Instance info panel — shows identity, children, clone button
#[component]
fn InstancePanel() -> impl IntoView {
    let (info, set_info) = create_signal(None::<serde_json::Value>);
    let (loading, set_loading) = create_signal(true);
    let (error, set_error) = create_signal(None::<String>);

    spawn_local(async move {
        let base = api::gateway_base_url();
        let url = format!("{}/instance/info", base);
        match gloo_net::http::Request::get(&url).send().await {
            Ok(resp) if resp.ok() => {
                if let Ok(data) = resp.json::<serde_json::Value>().await {
                    set_info.set(Some(data));
                }
            }
            Ok(resp) => {
                set_error.set(Some(format!("HTTP {}", resp.status())));
            }
            Err(e) => {
                set_error.set(Some(format!("{}", e)));
            }
        }
        set_loading.set(false);
    });

    view! {
        <div class="instance-panel">
            <h3>"Instance Info"</h3>

            <Show when=move || loading.get() fallback=|| ()>
                <p class="loading">"Loading instance info..."</p>
            </Show>

            <Show when=move || error.get().is_some() fallback=|| ()>
                <p class="error-text">"Instance info unavailable"</p>
            </Show>

            <Show when=move || info.get().is_some() fallback=|| ()>
                {move || {
                    let data = info.get().unwrap_or_default();

                    let identity = data.get("identity").cloned();
                    let children_count = data.get("children_count")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let clone_available = data.get("clone_available")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let clone_price = data.get("clone_price")
                        .and_then(|v| v.as_str())
                        .unwrap_or("N/A")
                        .to_string();
                    let version = data.get("version")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let uptime = data.get("uptime_seconds")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);

                    let address = identity.as_ref()
                        .and_then(|id| id.get("address"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("N/A")
                        .to_string();
                    let instance_id = identity.as_ref()
                        .and_then(|id| id.get("instance_id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("N/A")
                        .to_string();
                    let parent_url = identity.as_ref()
                        .and_then(|id| id.get("parent_url"))
                        .and_then(|v| v.as_str())
                        .map(String::from);

                    let children = data.get("children")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();

                    view! {
                        <div class="instance-details">
                            <div class="instance-identity">
                                <p><strong>"Address: "</strong><code>{address}</code></p>
                                <p><strong>"Instance: "</strong><code>{instance_id}</code></p>
                                <p><strong>"Version: "</strong>{version}</p>
                                <p><strong>"Uptime: "</strong>{format!("{}s", uptime)}</p>
                                {parent_url.map(|url| view! {
                                    <p><strong>"Parent: "</strong>
                                        <a href=url.clone() target="_blank">{url}</a>
                                    </p>
                                })}
                            </div>

                            <Show when=move || clone_available fallback=|| ()>
                                <div class="clone-section">
                                    <p>"Clone this instance for " <strong>{clone_price.clone()}</strong></p>
                                </div>
                            </Show>

                            <Show when=move || { children_count > 0 } fallback=|| ()>
                                <div class="children-list">
                                    <h4>{format!("Children ({})", children_count)}</h4>
                                    <ul>
                                        {children.iter().map(|child| {
                                            let child_id = child.get("instance_id")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("unknown")
                                                .to_string();
                                            let child_url = child.get("url")
                                                .and_then(|v| v.as_str())
                                                .map(String::from);
                                            let child_status = child.get("status")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("unknown")
                                                .to_string();
                                            view! {
                                                <li>
                                                    <code>{child_id}</code>
                                                    " — "
                                                    <span class="status-badge">{child_status}</span>
                                                    {child_url.map(|url| view! {
                                                        " "
                                                        <a href=url.clone() target="_blank">{url}</a>
                                                    })}
                                                </li>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </ul>
                                </div>
                            </Show>
                        </div>
                    }
                }}
            </Show>
        </div>
    }
}

/// Endpoint registration form component
#[component]
fn EndpointRegistration() -> impl IntoView {
    let (wallet, _) = expect_context::<(ReadSignal<WalletState>, WriteSignal<WalletState>)>();

    let (slug, set_slug) = create_signal(String::new());
    let (target_url, set_target_url) = create_signal(String::new());
    let (price, set_price) = create_signal(String::from("0.001"));
    let (_description, set_description) = create_signal(String::new());
    let (loading, set_loading) = create_signal(false);
    let (error, set_error) = create_signal(None::<String>);
    let (success, set_success) = create_signal(None::<String>);

    let slug_valid = move || {
        let s = slug.get();
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    };

    let url_valid = move || {
        let u = target_url.get();
        u.starts_with("https://") || u.starts_with("http://")
    };

    let price_valid = move || {
        let p = price.get();
        p.parse::<f64>().map(|v| v > 0.0).unwrap_or(false)
    };

    let can_submit = move || {
        wallet.get().connected && slug_valid() && url_valid() && price_valid() && !loading.get()
    };

    let do_register = move |_| {
        if !can_submit() {
            return;
        }

        set_loading.set(true);
        set_error.set(None);
        set_success.set(None);

        let s = slug.get();
        let u = target_url.get();
        let p = price.get();

        spawn_local(async move {
            match api::register_endpoint(&s, &u, &p).await {
                Ok(resp) => {
                    let gw_url = resp
                        .get("gateway_url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    set_success.set(Some(format!(
                        "Registered /g/{} -> {} (gateway: {})",
                        s, u, gw_url
                    )));
                    set_slug.set(String::new());
                    set_target_url.set(String::new());
                    set_price.set("0.001".to_string());
                    set_description.set(String::new());
                }
                Err(e) => {
                    set_error.set(Some(e));
                }
            }
            set_loading.set(false);
        });
    };

    view! {
        <div class="registration-card">
            <h3>"Register an Endpoint"</h3>
            <p class="registration-subtitle">
                "Expose any URL behind a paywall — clients pay per request via the x402 protocol"
            </p>

            <div class="registration-form">
                <div class="form-group">
                    <label>"Slug"</label>
                    <input
                        type="text"
                        placeholder="my-api"
                        prop:value=move || slug.get()
                        on:input=move |ev| set_slug.set(event_target_value(&ev))
                    />
                    {move || {
                        let s = slug.get();
                        if !s.is_empty() && !slug_valid() {
                            view! { <p class="input-error">"Only alphanumeric, hyphens, underscores"</p> }.into_view()
                        } else if !s.is_empty() {
                            view! { <p class="input-hint">{format!("Access via /g/{}", s)}</p> }.into_view()
                        } else {
                            view! { <p class="input-hint">"URL-safe identifier"</p> }.into_view()
                        }
                    }}
                </div>
                <div class="form-group">
                    <label>"Target URL"</label>
                    <input
                        type="text"
                        placeholder="https://api.example.com"
                        prop:value=move || target_url.get()
                        on:input=move |ev| set_target_url.set(event_target_value(&ev))
                    />
                    {move || {
                        let u = target_url.get();
                        if !u.is_empty() && !url_valid() {
                            view! { <p class="input-error">"Must start with https:// or http://"</p> }.into_view()
                        } else {
                            view! { <p class="input-hint">"Upstream URL to proxy requests to"</p> }.into_view()
                        }
                    }}
                </div>
                <div class="form-group">
                    <label>"Price (USD)"</label>
                    <input
                        type="text"
                        placeholder="0.001"
                        prop:value=move || price.get()
                        on:input=move |ev| set_price.set(event_target_value(&ev))
                    />
                    {move || {
                        if !price_valid() && !price.get().is_empty() {
                            view! { <p class="input-error">"Enter a positive number"</p> }.into_view()
                        } else {
                            view! { <p class="input-hint">"Per-request cost in pathUSD"</p> }.into_view()
                        }
                    }}
                </div>
            </div>

            <div class="registration-actions">
                <button
                    class="btn btn-primary"
                    on:click=do_register
                    disabled=move || !can_submit()
                >
                    {move || if loading.get() { "Registering..." } else { "Register Endpoint" }}
                </button>
                {move || {
                    if !wallet.get().connected {
                        view! { <span class="soul-muted">"Connect wallet to register"</span> }.into_view()
                    } else {
                        view! { <span></span> }.into_view()
                    }
                }}
            </div>

            <Show when=move || error.get().is_some() fallback=|| ()>
                <p class="error-text" style="margin-top: 12px">
                    {move || error.get().unwrap_or_default()}
                </p>
            </Show>

            <Show when=move || success.get().is_some() fallback=|| ()>
                <div class="registration-success" style="margin-top: 12px">
                    {move || success.get().unwrap_or_default()}
                </div>
            </Show>
        </div>
    }
}

/// Home page with payment demo
#[component]
fn HomePage() -> impl IntoView {
    view! {
        <div class="page">
            <h1>"x402 Payment Demo"</h1>
            <p class="subtitle">
                "HTTP 402 Payment Required — pay-per-request APIs on Tempo"
            </p>

            <PaymentDemo />
            <WalletManagement />
            <EndpointRegistration />
            <InstancePanel />

            <div class="info-section">
                <h2>"How it works"</h2>
                <ol class="steps">
                    <li>"Connect a wallet (MetaMask, demo key, or create an embedded wallet)"</li>
                    <li>"Click \"Pay & Request\" to hit a paid API endpoint"</li>
                    <li>"The gateway returns 402 with payment requirements"</li>
                    <li>"Your wallet signs an EIP-712 payment authorization"</li>
                    <li>"The request retries with PAYMENT-SIGNATURE header"</li>
                    <li>"The facilitator settles the payment on-chain"</li>
                    <li>"You get the API response + a transaction hash"</li>
                </ol>
            </div>
        </div>
    }
}

/// Payment demo component
#[component]
fn PaymentDemo() -> impl IntoView {
    let (wallet, _) = expect_context::<(ReadSignal<WalletState>, WriteSignal<WalletState>)>();
    let (status, set_status) = create_signal(String::from(
        "Ready — connect a wallet and click Pay & Request",
    ));
    let (result, set_result) = create_signal(None::<String>);
    let (tx_hash, set_tx_hash) = create_signal(None::<String>);
    let (loading, set_loading) = create_signal(false);

    let make_request = move |_| {
        let w = wallet.get();
        if !w.connected {
            set_status
                .set("Connect a wallet first (MetaMask, Demo Key, or Create Wallet)".to_string());
            return;
        }

        set_loading.set(true);
        set_status.set("Requesting /g/demo — expecting 402...".to_string());
        set_result.set(None);
        set_tx_hash.set(None);

        spawn_local(async move {
            match api::make_paid_request(&w).await {
                Ok((data, settle)) => {
                    if let Some(ref s) = settle {
                        if s.success {
                            set_status.set("Payment settled on-chain!".to_string());
                        } else {
                            set_status.set("Payment sent (check transaction)".to_string());
                        }
                        set_tx_hash.set(s.transaction.clone());
                    } else {
                        set_status.set("Response received (no settlement header)".to_string());
                    }
                    set_result.set(Some(data));
                }
                Err(e) => {
                    set_status.set(format!("Error: {}", e));
                }
            }
            set_loading.set(false);
        });
    };

    view! {
        <div class="demo-card">
            <h3>"Make a Paid Request"</h3>
            <p class="demo-description">
                "Calls " <code>"/g/demo"</code> " — a paid proxy to httpbin.org ($0.001 per request)"
            </p>

            <div class="demo-controls">
                <button
                    class="btn btn-primary"
                    on:click=make_request
                    disabled=move || loading.get()
                >
                    {move || if loading.get() { "Signing & paying..." } else { "Pay & Request ($0.001)" }}
                </button>
            </div>

            <div class="demo-status">
                <p class="status-text">{move || status.get()}</p>
            </div>

            <Show when=move || tx_hash.get().is_some() fallback=|| ()>
                <div class="demo-tx">
                    <h4>"Transaction"</h4>
                    <a
                        href=move || format!("https://explore.moderato.tempo.xyz/tx/{}", tx_hash.get().unwrap_or_default())
                        target="_blank"
                        class="tx-link"
                    >
                        {move || tx_hash.get().unwrap_or_default()}
                    </a>
                </div>
            </Show>

            <Show when=move || result.get().is_some() fallback=|| ()>
                <div class="demo-result">
                    <h4>"Proxied Response"</h4>
                    <pre class="code-block">{move || result.get().unwrap_or_default()}</pre>
                </div>
            </Show>
        </div>
    }
}

/// Format seconds into human-readable uptime string
fn format_uptime(secs: i64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d {}h", secs / 86400, (secs % 86400) / 3600)
    }
}

/// Dashboard page
#[component]
fn DashboardPage() -> impl IntoView {
    let (info, set_info) = create_signal(None::<serde_json::Value>);
    let (endpoints, set_endpoints) = create_signal(Vec::<serde_json::Value>::new());
    let (analytics, set_analytics) = create_signal(None::<serde_json::Value>);
    let (soul_status, set_soul_status) = create_signal(None::<serde_json::Value>);
    let (mind_status, set_mind_status) = create_signal(None::<serde_json::Value>);
    let (loading, set_loading) = create_signal(true);
    let (error, set_error) = create_signal(None::<String>);
    let (tick, set_tick) = create_signal(0u32);

    // Fetch all dashboard data
    let fetch_data = move || {
        spawn_local(async move {
            let base = api::gateway_base_url();

            // Fetch instance info
            match gloo_net::http::Request::get(&format!("{}/instance/info", base))
                .send()
                .await
            {
                Ok(resp) if resp.ok() => {
                    if let Ok(data) = resp.json::<serde_json::Value>().await {
                        set_info.set(Some(data));
                    }
                }
                Ok(resp) => {
                    set_error.set(Some(format!("HTTP {}", resp.status())));
                }
                Err(e) => {
                    set_error.set(Some(format!("{}", e)));
                }
            }

            // Fetch endpoints
            if let Ok(eps) = api::list_endpoints().await {
                set_endpoints.set(eps);
            }

            // Fetch analytics
            if let Ok(data) = api::fetch_analytics().await {
                set_analytics.set(Some(data));
            }

            // Fetch soul status
            if let Ok(data) = api::fetch_soul_status().await {
                set_soul_status.set(Some(data));
            }

            // Fetch mind status (graceful — 404 means not enabled)
            match api::fetch_mind_status().await {
                Ok(data) => set_mind_status.set(Some(data)),
                Err(_) => set_mind_status.set(None),
            }

            set_loading.set(false);
        });
    };

    // Initial fetch
    fetch_data();

    // Auto-refresh every 10s
    let interval = Interval::new(10_000, move || {
        set_tick.update(|t| *t = t.wrapping_add(1));
        fetch_data();
    });

    on_cleanup(move || {
        drop(interval);
    });

    view! {
        <div class="page dashboard">
            <div class="dashboard-header">
                <h1>"Network Dashboard"</h1>
                <div class="live-badge">
                    <span class="live-dot"></span>
                    "Live"
                    {move || {
                        let _ = tick.get();
                    }}
                </div>
            </div>

            <Show when=move || loading.get() && info.get().is_none() fallback=|| ()>
                <p class="loading">"Loading dashboard..."</p>
            </Show>

            <Show when=move || error.get().is_some() && info.get().is_none() fallback=|| ()>
                <p class="error-text">{move || error.get().unwrap_or_default()}</p>
            </Show>

            <Show when=move || info.get().is_some() fallback=|| ()>
                {move || {
                    let data = info.get().unwrap_or_default();

                    let version = data.get("version")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let uptime = data.get("uptime_seconds")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let eps = endpoints.get();
                    let ep_count = eps.len();

                    let analytics_data = analytics.get();
                    let total_payments = analytics_data.as_ref()
                        .and_then(|a| a.get("total_payments"))
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let total_revenue_usd = analytics_data.as_ref()
                        .and_then(|a| a.get("total_revenue_usd"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("$0")
                        .to_string();
                    let analytics_endpoints = analytics_data.as_ref()
                        .and_then(|a| a.get("endpoints"))
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let active_endpoints = analytics_endpoints.len();

                    view! {
                        // Stats cards
                        <div class="stats-grid">
                            <div class="stat-card">
                                <span class="stat-label">"Status"</span>
                                <span class="stat-value">
                                    <span class="status-dot status-dot--green"></span>
                                    " Online"
                                </span>
                            </div>
                            <div class="stat-card">
                                <span class="stat-label">"Version"</span>
                                <span class="stat-value">{format!("v{}", version)}</span>
                            </div>
                            <div class="stat-card">
                                <span class="stat-label">"Uptime"</span>
                                <span class="stat-value">{format_uptime(uptime)}</span>
                            </div>
                        </div>

                        // Analytics stats
                        <div class="stats-grid">
                            <div class="stat-card">
                                <span class="stat-label">"Total Payments"</span>
                                <span class="stat-value">{total_payments.to_string()}</span>
                            </div>
                            <div class="stat-card">
                                <span class="stat-label">"Total Revenue"</span>
                                <span class="stat-value">{total_revenue_usd}</span>
                            </div>
                            <div class="stat-card">
                                <span class="stat-label">"Active Endpoints"</span>
                                <span class="stat-value">{active_endpoints.to_string()}</span>
                            </div>
                        </div>

                        // Mind panel (if available), otherwise Soul panel
                        {move || {
                            let ms = mind_status.get();
                            if ms.is_some() {
                                view! {
                                    <MindPanel status=mind_status />
                                    <SoulPanel status=soul_status />
                                }.into_view()
                            } else {
                                view! {
                                    <SoulPanel status=soul_status />
                                }.into_view()
                            }
                        }}

                        // Endpoints table
                        <div class="endpoints-section">
                            <h2>{format!("Registered Endpoints ({})", ep_count)}</h2>
                            {if eps.is_empty() {
                                view! { <p class="empty">"No endpoints registered yet. Register one from the Demo page."</p> }.into_view()
                            } else {
                                let analytics_eps = analytics_endpoints.clone();
                                view! {
                                    <div class="endpoints-table">
                                        <div class="endpoint-row endpoint-header">
                                            <span class="endpoint-slug">"Endpoint"</span>
                                            <span class="endpoint-price">"Price"</span>
                                            <span class="endpoint-stat">"Calls"</span>
                                            <span class="endpoint-stat">"Payments"</span>
                                            <span class="endpoint-stat">"Revenue"</span>
                                            <span class="endpoint-desc">"Description"</span>
                                        </div>
                                        {eps.iter().map(|ep| {
                                            let slug = ep.get("slug")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("?")
                                                .to_string();
                                            let price = ep.get("price")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("?")
                                                .to_string();
                                            let description = ep.get("description")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("")
                                                .to_string();
                                            let gateway_url = ep.get("gateway_url")
                                                .and_then(|v| v.as_str())
                                                .map(String::from);

                                            let ep_stats = analytics_eps.iter().find(|a| {
                                                a.get("slug").and_then(|v| v.as_str()) == Some(&slug)
                                            });
                                            let calls = ep_stats
                                                .and_then(|s| s.get("request_count"))
                                                .and_then(|v| v.as_i64())
                                                .unwrap_or(0);
                                            let payments = ep_stats
                                                .and_then(|s| s.get("payment_count"))
                                                .and_then(|v| v.as_i64())
                                                .unwrap_or(0);
                                            let revenue = ep_stats
                                                .and_then(|s| s.get("revenue_usd"))
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("$0")
                                                .to_string();

                                            view! {
                                                <div class="endpoint-row">
                                                    <span class="endpoint-slug">
                                                        {gateway_url.as_ref().map(|url| view! {
                                                            <a href=url.clone() target="_blank">{format!("/g/{}", slug)}</a>
                                                        })}
                                                        {if gateway_url.is_none() {
                                                            Some(view! { <span>{format!("/g/{}", slug)}</span> })
                                                        } else {
                                                            None
                                                        }}
                                                    </span>
                                                    <span class="endpoint-price">{format!("${}", price)}</span>
                                                    <span class="endpoint-stat">{calls.to_string()}</span>
                                                    <span class="endpoint-stat">{payments.to_string()}</span>
                                                    <span class="endpoint-stat">{revenue}</span>
                                                    <span class="endpoint-desc">{description}</span>
                                                </div>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                }.into_view()
                            }}
                        </div>
                    }
                }}
            </Show>
        </div>
    }
}

/// Format a unix timestamp as relative time (e.g., "2m ago")
fn format_relative_time(unix_ts: i64) -> String {
    let now = (js_sys::Date::now() / 1000.0) as i64;
    let diff = now - unix_ts;
    if diff < 0 {
        return "just now".to_string();
    }
    if diff < 60 {
        format!("{}s ago", diff)
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

/// Format a unix timestamp as HH:MM
fn format_timestamp(unix_ts: i64) -> String {
    let date = js_sys::Date::new_0();
    date.set_time((unix_ts as f64) * 1000.0);
    let h = date.get_hours();
    let m = date.get_minutes();
    format!("{:02}:{:02}", h, m)
}

/// Mind status panel — dual-hemisphere display
#[component]
fn MindPanel(status: ReadSignal<Option<serde_json::Value>>) -> impl IntoView {
    view! {
        <div class="mind-section">
            {move || {
                let data = match status.get() {
                    Some(d) => d,
                    None => return view! { <span></span> }.into_view(),
                };

                let left = data.get("left").cloned();
                let right = data.get("right").cloned();

                let render_hemisphere = |hemi: Option<serde_json::Value>, side: &str| {
                    let data = hemi.unwrap_or_default();
                    let active = data.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
                    let cycles = data.get("total_cycles").and_then(|v| v.as_u64()).unwrap_or(0);
                    let last_think = data.get("last_think_at")
                        .and_then(|v| v.as_i64())
                        .map(format_relative_time)
                        .unwrap_or_else(|| "never".to_string());
                    let last_thought = data.get("recent_thoughts")
                        .and_then(|v| v.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|t| t.get("content"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let truncated = if last_thought.len() > 100 {
                        format!("{}...", &last_thought[..100])
                    } else {
                        last_thought
                    };

                    let (badge_class, badge_text) = if active {
                        ("soul-status--green", "Active")
                    } else {
                        ("soul-status--gray", "Inactive")
                    };

                    let hemisphere_class = format!("mind-hemisphere mind-hemisphere--{}", side);
                    let title_class = format!("hemisphere-title hemisphere-title--{}", side);
                    let label = if side == "left" { "Left (Analytical)" } else { "Right (Holistic)" };

                    view! {
                        <div class=hemisphere_class>
                            <div class="hemisphere-header">
                                <span class=title_class>{label}</span>
                                <span class={format!("soul-status-badge {}", badge_class)}>
                                    {badge_text}
                                </span>
                            </div>
                            <div class="hemisphere-stat">
                                <span class="hemisphere-stat-label">"Cycles"</span>
                                <span class="hemisphere-stat-value">{cycles.to_string()}</span>
                            </div>
                            <div class="hemisphere-stat">
                                <span class="hemisphere-stat-label">"Last thought"</span>
                                <span class="hemisphere-stat-value">{last_think}</span>
                            </div>
                            {if !truncated.is_empty() {
                                Some(view! {
                                    <div class="hemisphere-thought">{truncated}</div>
                                })
                            } else {
                                None
                            }}
                        </div>
                    }
                };

                view! {
                    <div class="mind-card">
                        <div class="mind-header">
                            <h2>"Mind"</h2>
                            <span class="soul-status-badge soul-status--green">"Enabled"</span>
                        </div>
                        <div class="mind-hemispheres">
                            {render_hemisphere(left, "left")}
                            <div class="mind-callosum">
                                <div class="callosum-line"></div>
                                <span class="callosum-label">"Callosum"</span>
                                <div class="callosum-line"></div>
                            </div>
                            {render_hemisphere(right, "right")}
                        </div>
                    </div>
                }.into_view()
            }}
        </div>
    }
}

/// Soul observability panel — enhanced with mode, flags, expandable thoughts
#[component]
fn SoulPanel(status: ReadSignal<Option<serde_json::Value>>) -> impl IntoView {
    let (expanded_idx, set_expanded_idx) = create_signal(None::<usize>);

    view! {
        <div class="soul-section">
            {move || {
                let data = match status.get() {
                    Some(d) => d,
                    None => return view! {
                        <div class="soul-card soul-card--inactive">
                            <div class="soul-header">
                                <h2>"Soul"</h2>
                                <span class="soul-status-badge soul-status--gray">"No Data"</span>
                            </div>
                            <p class="soul-muted">"Soul status unavailable"</p>
                        </div>
                    }.into_view(),
                };

                let active = data.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
                let dormant = data.get("dormant").and_then(|v| v.as_bool()).unwrap_or(false);
                let total_cycles = data.get("total_cycles").and_then(|v| v.as_u64()).unwrap_or(0);
                let last_think_at = data.get("last_think_at").and_then(|v| v.as_i64());
                let thoughts = data.get("recent_thoughts")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                // Mode from status response
                let mode = data.get("mode")
                    .and_then(|v| v.as_str())
                    .unwrap_or(if !active { "inactive" } else if dormant { "dormant" } else { "observe" })
                    .to_string();

                let tools_enabled = data.get("tools_enabled").and_then(|v| v.as_bool()).unwrap_or(false);
                let coding_enabled = data.get("coding_enabled").and_then(|v| v.as_bool()).unwrap_or(false);

                let (badge_class, badge_label) = if !active {
                    ("soul-status--gray", "Inactive")
                } else if dormant {
                    ("soul-status--yellow", "Dormant")
                } else {
                    ("soul-status--green", "Active")
                };

                let mode_class = match mode.as_str() {
                    "observe" => "soul-mode--observe",
                    "chat" => "soul-mode--chat",
                    "code" => "soul-mode--code",
                    "review" => "soul-mode--review",
                    _ => "soul-mode--observe",
                };

                let last_thought_str = last_think_at
                    .map(format_relative_time)
                    .unwrap_or_else(|| "never".to_string());

                view! {
                    <div class="soul-card">
                        <div class="soul-header">
                            <h2>"Soul"</h2>
                            <div class="soul-header-badges">
                                {if active && !dormant {
                                    Some(view! {
                                        <span class={format!("soul-mode-badge {}", mode_class)}>
                                            {mode.clone()}
                                        </span>
                                    })
                                } else {
                                    None
                                }}
                                <span class={format!("soul-status-badge {}", badge_class)}>
                                    {badge_label}
                                </span>
                            </div>
                        </div>

                        <div class="stats-grid">
                            <div class="stat-card">
                                <span class="stat-label">"Cycles"</span>
                                <span class="stat-value">{total_cycles.to_string()}</span>
                            </div>
                            <div class="stat-card">
                                <span class="stat-label">"Last Thought"</span>
                                <span class="stat-value">{last_thought_str}</span>
                            </div>
                            <div class="stat-card">
                                <span class="stat-label">"Mode"</span>
                                <span class="stat-value">{mode.clone()}</span>
                            </div>
                        </div>

                        // Feature flags
                        {if active {
                            Some(view! {
                                <div class="soul-flags">
                                    <span class={if tools_enabled { "soul-flag soul-flag--on" } else { "soul-flag" }}>
                                        {if tools_enabled { "tools: on" } else { "tools: off" }}
                                    </span>
                                    <span class={if coding_enabled { "soul-flag soul-flag--on" } else { "soul-flag" }}>
                                        {if coding_enabled { "coding: on" } else { "coding: off" }}
                                    </span>
                                </div>
                            })
                        } else {
                            None
                        }}

                        {if thoughts.is_empty() && !active {
                            view! {
                                <p class="soul-muted">"Soul not active"</p>
                            }.into_view()
                        } else if thoughts.is_empty() {
                            view! {
                                <p class="soul-muted">"No thoughts recorded yet"</p>
                            }.into_view()
                        } else {
                            let expanded = expanded_idx.get();
                            view! {
                                <div class="soul-thoughts">
                                    <h3>"Recent Thoughts"</h3>
                                    {thoughts.iter().enumerate().map(|(idx, t)| {
                                        let thought_type = t.get("type")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("unknown")
                                            .to_string();
                                        let content = t.get("content")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string();
                                        let created_at = t.get("created_at")
                                            .and_then(|v| v.as_i64())
                                            .unwrap_or(0);

                                        let badge_abbr = match thought_type.as_str() {
                                            "observation" => "obs",
                                            "reasoning" => "reason",
                                            "decision" => "decide",
                                            "reflection" => "reflect",
                                            "mutation" => "mutate",
                                            "cross_hemisphere" => "cross",
                                            "escalation" => "escalate",
                                            "memory_consolidation" => "memory",
                                            _ => &thought_type,
                                        };

                                        let is_expanded = expanded == Some(idx);
                                        let display_content = if is_expanded || content.len() <= 120 {
                                            content.clone()
                                        } else {
                                            format!("{}...", &content[..120])
                                        };
                                        let is_truncatable = content.len() > 120;
                                        let content_class = if is_expanded {
                                            "thought-content thought-content--expanded"
                                        } else {
                                            "thought-content"
                                        };

                                        view! {
                                            <div
                                                class="soul-thought"
                                                on:click=move |_| {
                                                    if is_truncatable {
                                                        set_expanded_idx.set(
                                                            if expanded == Some(idx) { None } else { Some(idx) }
                                                        );
                                                    }
                                                }
                                            >
                                                <span class={format!("thought-badge thought-badge--{}", thought_type)}>
                                                    {badge_abbr.to_string()}
                                                </span>
                                                <div class=content_class>
                                                    {display_content}
                                                    {if is_truncatable && !is_expanded {
                                                        Some(view! { <div class="thought-expand-hint">"click to expand"</div> })
                                                    } else {
                                                        None
                                                    }}
                                                </div>
                                                <span class="thought-time">{format_relative_time(created_at)}</span>
                                            </div>
                                        }
                                    }).collect::<Vec<_>>()}
                                </div>
                            }.into_view()
                        }}
                    </div>
                }.into_view()
            }}
        </div>
    }
}

/// Chat display message for the UI
#[derive(Clone, Debug)]
struct ChatDisplayMessage {
    role: &'static str, // "user" or "soul"
    content: String,
    tool_executions: Vec<serde_json::Value>,
    timestamp: i64,
    hemisphere: Option<String>,
}

/// Interactive chat page with mind/soul toggle
#[component]
fn ChatPage() -> impl IntoView {
    let (messages, set_messages) = create_signal(Vec::<ChatDisplayMessage>::new());
    let (input, set_input) = create_signal(String::new());
    let (loading, set_loading) = create_signal(false);
    let (error, set_error) = create_signal(None::<String>);
    let (use_mind, set_use_mind) = create_signal(false);
    let (mind_available, set_mind_available) = create_signal(false);

    // Check if mind is available on mount
    spawn_local(async move {
        if api::fetch_mind_status().await.is_ok() {
            set_mind_available.set(true);
        }
    });

    // Auto-scroll ref
    let messages_ref = create_node_ref::<html::Div>();

    let scroll_to_bottom = move || {
        if let Some(el) = messages_ref.get() {
            el.set_scroll_top(el.scroll_height());
        }
    };

    let now_ts = move || (js_sys::Date::now() / 1000.0) as i64;

    let do_send = move || {
        let msg = input.get().trim().to_string();
        if msg.is_empty() || loading.get() {
            return;
        }

        let ts = now_ts();
        set_messages.update(|msgs| {
            msgs.push(ChatDisplayMessage {
                role: "user",
                content: msg.clone(),
                tool_executions: vec![],
                timestamp: ts,
                hemisphere: None,
            });
        });
        set_input.set(String::new());
        set_loading.set(true);
        set_error.set(None);

        let is_mind = use_mind.get();

        spawn_local(async move {
            let result = if is_mind {
                api::send_mind_chat(&msg).await
            } else {
                api::send_soul_chat(&msg).await
            };

            match result {
                Ok(resp) => {
                    let reply = resp
                        .get("reply")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let tools = resp
                        .get("tool_executions")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let hemisphere = resp
                        .get("hemisphere")
                        .and_then(|v| v.as_str())
                        .map(String::from);

                    let ts = (js_sys::Date::now() / 1000.0) as i64;
                    set_messages.update(|msgs| {
                        msgs.push(ChatDisplayMessage {
                            role: "soul",
                            content: reply,
                            tool_executions: tools,
                            timestamp: ts,
                            hemisphere,
                        });
                    });
                }
                Err(e) => {
                    set_error.set(Some(e));
                }
            }
            set_loading.set(false);
            scroll_to_bottom();
        });

        // Scroll after adding user message
        scroll_to_bottom();
    };

    let clear_chat = move |_| {
        set_messages.set(Vec::new());
        set_error.set(None);
    };

    let on_click = move |_: web_sys::MouseEvent| {
        do_send();
    };

    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Enter" && !ev.shift_key() {
            ev.prevent_default();
            do_send();
        }
    };

    view! {
        <div class="chat-page">
            <div class="chat-header">
                <h1>{move || if use_mind.get() { "Mind Chat" } else { "Soul Chat" }}</h1>
                <div class="chat-header-controls">
                    <div class="chat-tabs">
                        <button
                            class=move || if !use_mind.get() { "chat-tab active" } else { "chat-tab" }
                            on:click=move |_| set_use_mind.set(false)
                        >
                            "Soul"
                        </button>
                        <Show when=move || mind_available.get() fallback=|| ()>
                            <button
                                class=move || if use_mind.get() { "chat-tab active" } else { "chat-tab" }
                                on:click=move |_| set_use_mind.set(true)
                            >
                                "Mind"
                            </button>
                        </Show>
                    </div>
                    <button class="chat-clear-btn" on:click=clear_chat>"Clear"</button>
                </div>
            </div>

            <div class="chat-messages" node_ref=messages_ref>
                <For
                    each=move || {
                        let msgs = messages.get();
                        msgs.into_iter().enumerate().collect::<Vec<_>>()
                    }
                    key=|(i, _)| *i
                    children=move |(_, msg)| {
                        let class = format!("chat-message {}", msg.role);
                        let tools = msg.tool_executions.clone();
                        let ts = msg.timestamp;
                        let hemisphere = msg.hemisphere.clone();
                        view! {
                            <div class=class>
                                <div class="chat-message-header">
                                    <div style="display:flex;gap:6px;align-items:center">
                                        <span class="chat-message-role">
                                            {if msg.role == "user" { "You" } else { "Soul" }}
                                        </span>
                                        {hemisphere.map(|h| view! {
                                            <span class="chat-hemisphere-tag">{h}</span>
                                        })}
                                    </div>
                                    <span class="chat-message-time">{format_timestamp(ts)}</span>
                                </div>
                                <div class="chat-message-content">
                                    {msg.content.clone()}
                                </div>
                                {if !tools.is_empty() {
                                    Some(view! {
                                        <div class="chat-tools">
                                            {tools.into_iter().map(|t| {
                                                let cmd = t.get("command")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("?")
                                                    .to_string();
                                                let stdout = t.get("stdout")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("")
                                                    .to_string();
                                                let stderr = t.get("stderr")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("")
                                                    .to_string();
                                                let exit_code = t.get("exit_code")
                                                    .and_then(|v| v.as_i64())
                                                    .unwrap_or(-1);
                                                let (expanded, set_expanded) = create_signal(false);
                                                view! {
                                                    <div class="chat-tool-block">
                                                        <button
                                                            class="chat-tool-header"
                                                            on:click=move |_| set_expanded.update(|v| *v = !*v)
                                                        >
                                                            <span class="chat-tool-cmd">"$ " {cmd.clone()}</span>
                                                            <span class="chat-tool-exit">
                                                                {format!("exit {}", exit_code)}
                                                            </span>
                                                        </button>
                                                        <Show when=move || expanded.get() fallback=|| ()>
                                                            <pre class="chat-tool-output">
                                                                {if !stdout.is_empty() {
                                                                    stdout.clone()
                                                                } else if !stderr.is_empty() {
                                                                    stderr.clone()
                                                                } else {
                                                                    "(no output)".to_string()
                                                                }}
                                                            </pre>
                                                        </Show>
                                                    </div>
                                                }
                                            }).collect_view()}
                                        </div>
                                    })
                                } else {
                                    None
                                }}
                            </div>
                        }
                    }
                />

                <Show when=move || loading.get() fallback=|| ()>
                    <div class="chat-message soul">
                        <div class="chat-message-role">"Soul"</div>
                        <div class="chat-message-content chat-typing">"Thinking..."</div>
                    </div>
                </Show>

                <Show when=move || error.get().is_some() fallback=|| ()>
                    <div class="chat-error">
                        {move || error.get().unwrap_or_default()}
                    </div>
                </Show>
            </div>

            <div class="chat-input-bar">
                <input
                    type="text"
                    class="chat-input"
                    placeholder=move || {
                        if use_mind.get() {
                            "Ask the mind something..."
                        } else {
                            "Ask the soul something..."
                        }
                    }
                    prop:value=move || input.get()
                    on:input=move |ev| set_input.set(event_target_value(&ev))
                    on:keydown=on_keydown
                    disabled=move || loading.get()
                />
                <button
                    class="btn btn-primary"
                    on:click=on_click
                    disabled=move || loading.get() || input.get().trim().is_empty()
                >
                    {move || if loading.get() { "Sending..." } else { "Send" }}
                </button>
            </div>
        </div>
    }
}

/// Footer with version
#[component]
fn Footer() -> impl IntoView {
    view! {
        <footer class="footer">
            <p>
                "Built with "
                <a href="https://github.com/compusophy/tempo-x402">"tempo-x402"</a>
                " on Tempo Moderato"
            </p>
            <p class="footer-version">"v1.1.0"</p>
        </footer>
    }
}

/// 404 page
#[component]
fn NotFound() -> impl IntoView {
    view! {
        <div class="page">
            <h1>"404 - Not Found"</h1>
            <p><a href="/">"Go home"</a></p>
        </div>
    }
}

/// Initialize the app
#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(|| view! { <App /> });
}
