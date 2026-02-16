#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::Router;
    use eventfold::EventLog;
    use leptos::prelude::*;
    use leptos_axum::{generate_route_list, LeptosRoutes};
    use std::sync::{Arc, Mutex};
    use todo_app::app::*;
    use todo_app::server::{AppLog, SendEventLog};
    use todo_app::state::{stats_reducer, todo_reducer, StatsState, TodoState};

    let log = EventLog::builder("./data")
        .max_log_size(10_000_000)
        .view::<TodoState>("todos", todo_reducer)
        .view::<StatsState>("stats", stats_reducer)
        .open()
        .expect("failed to open event log");

    let log: AppLog = Arc::new(Mutex::new(SendEventLog(log)));

    let conf = get_configuration(None).unwrap();
    let leptos_options = conf.leptos_options;
    let addr = leptos_options.site_addr;
    let routes = generate_route_list(App);

    let log_ctx = log.clone();
    let app = Router::new()
        .leptos_routes_with_context(
            &leptos_options,
            routes,
            move || {
                provide_context(log_ctx.clone());
            },
            {
                let leptos_options = leptos_options.clone();
                move || shell(leptos_options.clone())
            },
        )
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(leptos_options);

    println!("listening on http://{}", &addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

#[cfg(not(feature = "ssr"))]
pub fn main() {
    // Client-side main function is not used.
    // See lib.rs for the hydration entry point.
}
