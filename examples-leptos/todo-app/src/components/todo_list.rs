use crate::components::todo_item::TodoItem;
use crate::server::{get_todos, AddTodo, DeleteTodo, ToggleTodo};
use leptos::form::ActionForm;
use leptos::prelude::*;

#[component]
pub fn TodoList(version: RwSignal<u32>) -> impl IntoView {
    let add_action = ServerAction::<AddTodo>::new();
    let toggle_action = ServerAction::<ToggleTodo>::new();
    let delete_action = ServerAction::<DeleteTodo>::new();

    let add_v = add_action.version();
    let toggle_v = toggle_action.version();
    let delete_v = delete_action.version();

    // Bump the shared version signal when any action completes,
    // so the Stats component also refetches.
    Effect::new(move || {
        let sum = add_v.get() + toggle_v.get() + delete_v.get();
        if sum > 0 {
            version.update(|n| *n += 1);
        }
    });

    let todos = Resource::new(move || version.get(), |_| get_todos());

    view! {
        <section class="todo-section">
            <h2>"Todos"</h2>
            <ActionForm action=add_action>
                <div class="todo-form">
                    <input type="text" name="text" placeholder="What needs to be done?" required />
                    <button type="submit">"Add"</button>
                </div>
            </ActionForm>
            <Suspense fallback=move || view! { <p class="loading">"Loading..."</p> }>
                {move || Suspend::new(async move {
                    match todos.await {
                        Ok(state) => {
                            if state.items.is_empty() {
                                view! { <p class="empty">"No todos yet. Add one above!"</p> }.into_any()
                            } else {
                                view! {
                                    <ul class="todo-list">
                                        {state.items.into_iter().map(|todo| {
                                            view! { <TodoItem todo toggle_action delete_action /> }
                                        }).collect_view()}
                                    </ul>
                                }.into_any()
                            }
                        }
                        Err(e) => view! { <p class="error">"Error: " {e.to_string()}</p> }.into_any(),
                    }
                })}
            </Suspense>
        </section>
    }
}
