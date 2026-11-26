use serde_json::{json, Value};
use std::path::Path;

/// The Memo tab as plain text for an agent, with the paths of attached pictures so it can open them.
pub fn as_text(doc: &Value, pictures_dir: &Path) -> String {
    let pictures = |value: &Value| -> String {
        let list: Vec<String> = value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|one| one.as_str())
            .map(|name| pictures_dir.join(name).to_string_lossy().into_owned())
            .collect();
        if list.is_empty() { String::new() } else { format!(" (pictures: {})", list.join(", ")) }
    };
    let mut out = Vec::new();
    let todos = doc["todos"].as_array().cloned().unwrap_or_default();
    if !todos.is_empty() {
        out.push("To-dos:".to_string());
        for one in &todos {
            let status = status_of(one);
            let mark = if status == "done" { "x" } else { " " };
            let mut tags = Vec::new();
            if status == "doing" {
                tags.push("doing".to_string());
            }
            if let Some(who) = one["assignee"].as_str().filter(|who| !who.is_empty()) {
                tags.push(who.to_string());
            }
            let tags = if tags.is_empty() { String::new() } else { format!(" [{}]", tags.join(" ")) };
            out.push(format!("- [{mark}] {}{tags} (id {}){}", one["text"].as_str().unwrap_or(""), one["id"].as_str().unwrap_or(""), pictures(&one["images"])));
        }
    }
    let mut memos = doc["memos"].as_array().cloned().unwrap_or_default();
    // The first version kept one note in `notes` and `images`.
    if doc.get("memos").is_none() && (doc["notes"].as_str().is_some_and(|one| !one.trim().is_empty()) || doc["images"].as_array().is_some_and(|one| !one.is_empty())) {
        memos.push(json!({ "title": "", "text": doc["notes"], "images": doc["images"] }));
    }
    for one in &memos {
        let title = one["title"].as_str().filter(|one| !one.trim().is_empty()).unwrap_or("(untitled memo)");
        out.push(format!("\n## {title}{}\n{}", pictures(&one["images"]), one["text"].as_str().unwrap_or("").trim()));
    }
    if out.is_empty() { "The Memo tab is empty.".into() } else { out.join("\n").trim().to_string() }
}

/// A to-do's board column (F3): `todo`, `doing` or `done`. Files from before the board only have `done`.
pub fn status_of(todo: &Value) -> &'static str {
    match todo["status"].as_str() {
        Some("doing") => "doing",
        Some("done") => "done",
        Some("todo") => "todo",
        _ if todo["done"].as_bool() == Some(true) => "done",
        _ => "todo",
    }
}

/// What an agent asks of the to-do list. `by` is who asks (`AI`, `@amy's AI`).
pub struct TodoChange<'a> {
    pub action: &'a str,
    pub todo: Option<&'a str>,
    pub words: Option<&'a str>,
    pub status: Option<&'a str>,
    pub assignee: Option<&'a str>,
    pub by: &'a str,
}

/// Adds a to-do, marks one done or open, moves it on the board (`status`) or gives it to someone (`assign`);
/// returns what happened in a sentence. `done` always follows `status`, so an older app still reads it.
pub fn apply_todo(doc: &mut Value, change: &TodoChange, stamp: u128) -> Result<String, String> {
    if !doc.is_object() {
        *doc = json!({});
    }
    if !doc["todos"].is_array() {
        doc["todos"] = json!([]);
    }
    let list = doc["todos"].as_array_mut().expect("todos is an array");
    match change.action {
        "add" => {
            let words = change.words.ok_or("Give the to-do text.")?;
            let mut one = json!({ "id": format!("{stamp}-agent"), "text": words, "done": false, "status": "todo", "images": [], "by": change.by, "updated": stamp as u64 });
            if let Some(who) = change.assignee.filter(|who| !who.is_empty()) {
                one["assignee"] = Value::String(who.to_string());
            }
            list.push(one);
            Ok(format!("Added the to-do: {words}"))
        }
        "done" | "open" | "status" | "assign" => {
            let wanted = change.todo.ok_or("Give the to-do id from read_memo.")?;
            let one = list.iter_mut().find(|one| one["id"].as_str() == Some(wanted)).ok_or_else(|| format!("There is no to-do with id {wanted}."))?;
            let text = one["text"].as_str().unwrap_or("").to_string();
            let said = if change.action == "assign" {
                let who = change.assignee.unwrap_or("").trim();
                one["assignee"] = Value::String(who.to_string());
                if who.is_empty() { format!("\"{text}\" is no one's now.") } else { format!("Gave \"{text}\" to {who}.") }
            } else {
                let column = match change.action {
                    "done" => "done",
                    "open" => "todo",
                    _ => match change.status {
                        Some(column @ ("todo" | "doing" | "done")) => column,
                        _ => return Err("status must be todo, doing or done.".into()),
                    },
                };
                one["status"] = Value::String(column.into());
                one["done"] = Value::Bool(column == "done");
                format!("Moved \"{text}\" to {column}.")
            };
            one["updated"] = json!(stamp as u64);
            Ok(said)
        }
        _ => Err("action must be add, done, open, status or assign.".into()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn lists_todos_memos_and_the_old_single_note() {
        let dir = Path::new("pics");
        let text = super::as_text(&json!({ "todos": [{ "id": "a", "text": "Fix spawn", "done": true, "images": ["1.png"] }], "notes": "old note" }), dir);
        assert!(text.contains("- [x] Fix spawn (id a) (pictures: "));
        assert!(text.contains("1.png"));
        assert!(text.contains("## (untitled memo)\nold note"));
        assert_eq!(super::as_text(&json!({}), dir), "The Memo tab is empty.");
    }

    fn ask<'a>(action: &'a str, todo: Option<&'a str>, words: Option<&'a str>) -> super::TodoChange<'a> {
        super::TodoChange { action, todo, words, status: None, assignee: None, by: "AI" }
    }

    #[test]
    fn adds_and_checks_off_todos() {
        let mut doc = json!({ "memos": [] });
        super::apply_todo(&mut doc, &ask("add", None, Some("Update Essentials")), 7).unwrap();
        assert_eq!(doc["todos"][0]["id"], "7-agent");
        super::apply_todo(&mut doc, &ask("done", Some("7-agent"), None), 0).unwrap();
        assert_eq!(doc["todos"][0]["done"], true);
        assert_eq!(doc["todos"][0]["status"], "done");
        assert!(super::apply_todo(&mut doc, &ask("done", Some("nope"), None), 0).is_err());
        assert!(super::apply_todo(&mut doc, &ask("remove", None, None), 0).is_err());
        assert!(doc["memos"].is_array());
    }

    #[test]
    fn update_todo_assigns() {
        let mut doc = json!({ "todos": [{ "id": "a", "text": "Golem model", "done": false, "images": [] }] });
        let assign = super::TodoChange { action: "assign", todo: Some("a"), words: None, status: None, assignee: Some("@amy"), by: "@amy's AI" };
        super::apply_todo(&mut doc, &assign, 1).unwrap();
        let doing = super::TodoChange { action: "status", todo: Some("a"), words: None, status: Some("doing"), assignee: None, by: "@amy's AI" };
        super::apply_todo(&mut doc, &doing, 2).unwrap();
        assert_eq!(doc["todos"][0]["assignee"], "@amy");
        assert_eq!(doc["todos"][0]["status"], "doing");
        assert_eq!(doc["todos"][0]["done"], false);
        assert!(super::as_text(&doc, Path::new("p")).contains("- [ ] Golem model [doing @amy] (id a)"));
        let later = super::TodoChange { action: "status", todo: Some("a"), words: None, status: Some("later"), assignee: None, by: "AI" };
        assert!(super::apply_todo(&mut doc, &later, 3).is_err());
    }

    #[test]
    fn reads_old_todos_as_status() {
        assert_eq!(super::status_of(&json!({ "done": true })), "done");
        assert_eq!(super::status_of(&json!({ "done": false })), "todo");
        assert_eq!(super::status_of(&json!({ "done": false, "status": "doing" })), "doing");
    }
}
