use crate::server::get_stats;
use leptos::prelude::*;

#[component]
pub fn Stats(version: RwSignal<u32>) -> impl IntoView {
    let stats = Resource::new(move || version.get(), |_| get_stats());

    view! {
        <aside class="stats-section">
            <h2>"Statistics"</h2>
            <Suspense fallback=move || view! { <p class="loading">"Loading..."</p> }>
                {move || Suspend::new(async move {
                    match stats.await {
                        Ok(s) => {
                            let rate = if s.total_created > 0 {
                                (s.total_completed as f64 / s.total_created as f64 * 100.0) as u64
                            } else {
                                0
                            };
                            view! {
                                <dl class="stats-list">
                                    <div class="stat">
                                        <dt>"Created"</dt>
                                        <dd>{s.total_created}</dd>
                                    </div>
                                    <div class="stat">
                                        <dt>"Completed"</dt>
                                        <dd>{s.total_completed}</dd>
                                    </div>
                                    <div class="stat">
                                        <dt>"Deleted"</dt>
                                        <dd>{s.total_deleted}</dd>
                                    </div>
                                    <div class="stat">
                                        <dt>"Completion Rate"</dt>
                                        <dd>{rate}"%"</dd>
                                    </div>
                                </dl>
                            }.into_any()
                        }
                        Err(e) => view! { <p class="error">"Error: " {e.to_string()}</p> }.into_any(),
                    }
                })}
            </Suspense>
        </aside>
    }
}
