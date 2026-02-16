use crate::server::{DeleteTodo, ToggleTodo};
use crate::state::Todo;
use leptos::form::ActionForm;
use leptos::prelude::*;

#[component]
pub fn TodoItem(
    todo: Todo,
    toggle_action: ServerAction<ToggleTodo>,
    delete_action: ServerAction<DeleteTodo>,
) -> impl IntoView {
    let id_toggle = todo.id.clone();
    let id_delete = todo.id.clone();

    view! {
        <li class="todo-item" class:completed=todo.done>
            <ActionForm action=toggle_action>
                <input type="hidden" name="id" value=id_toggle />
                <button type="submit" class="toggle-btn">
                    {if todo.done { "\u{2713}" } else { "\u{25CB}" }}
                </button>
            </ActionForm>
            <span class="todo-text">{todo.text}</span>
            <ActionForm action=delete_action>
                <input type="hidden" name="id" value=id_delete />
                <button type="submit" class="delete-btn">"\u{00D7}"</button>
            </ActionForm>
        </li>
    }
}
