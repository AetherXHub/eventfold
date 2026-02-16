use crate::components::stats::Stats;
use crate::components::todo_list::TodoList;
use leptos::prelude::*;
use leptos_meta::{provide_meta_context, MetaTags, Stylesheet, Title};
use leptos_router::{
    components::{Route, Router, Routes},
    StaticSegment,
};

pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <AutoReload options=options.clone() />
                <HydrationScripts options/>
                <MetaTags/>
            </head>
            <body>
                <App/>
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/todo-app.css"/>
        <Title text="Todo App \u{2014} eventfold"/>
        <Router>
            <main>
                <Routes fallback=|| "Page not found.".into_view()>
                    <Route path=StaticSegment("") view=HomePage/>
                </Routes>
            </main>
        </Router>
    }
}

#[component]
fn HomePage() -> impl IntoView {
    let version = RwSignal::new(0u32);

    view! {
        <div class="app">
            <h1>"Todo App"</h1>
            <p class="subtitle">"Powered by eventfold \u{2014} no database, just events"</p>
            <div class="layout">
                <TodoList version/>
                <Stats version/>
            </div>
        </div>
    }
}
