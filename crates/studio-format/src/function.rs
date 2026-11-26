//! Functions (`function` files): a named action list any other list runs with `{"type": "function", "function": "<id>"}`,
//! so the same steps don't have to be copied into every rule, command and button. Both add-ons put the function's
//! steps in place of that step when a run starts (same placeholders and player as the list that called it), up to
//! `MOST_DEPTH` functions inside functions and the run's usual step limit.

use serde_json::{Map, Value};

type Data = Map<String, Value>;

/// Functions inside functions, at most; deeper calls are left out (that also stops a function calling itself forever).
pub const MOST_DEPTH: usize = 8;

/// The run's steps with every `function` step replaced by that function's `actions`. `actions_of` gives a function's
/// list by id (`None` for a missing or turned-off one, which is then skipped). At most `most` steps come out.
pub fn expand(steps: &[Value], actions_of: &dyn Fn(&str) -> Option<Vec<Value>>, most: usize) -> Vec<Value> {
    let mut out = Vec::new();
    expand_into(steps, actions_of, most, 0, &mut out);
    out
}

fn expand_into(steps: &[Value], actions_of: &dyn Fn(&str) -> Option<Vec<Value>>, most: usize, depth: usize, out: &mut Vec<Value>) {
    for one in steps {
        if out.len() >= most {
            return;
        }
        let Some(map) = one.as_object() else { continue };
        if map.get("type").and_then(Value::as_str) != Some("function") {
            out.push(one.clone());
            continue;
        }
        if depth >= MOST_DEPTH {
            continue;
        }
        let id = map.get("function").and_then(Value::as_str).unwrap_or_default().trim();
        if let Some(inner) = actions_of(id) {
            expand_into(&inner, actions_of, most, depth + 1, out);
        }
    }
}

/// A function file's steps.
pub fn actions(data: &Data) -> Vec<Value> {
    data.get("actions").and_then(Value::as_array).map(|list| list.iter().filter(|one| one.is_object()).cloned().collect()).unwrap_or_default()
}

pub fn problems(id: &str, data: &Data) -> Vec<String> {
    let mut out = Vec::new();
    let list = actions(data);
    if list.is_empty() {
        out.push("A function needs \"actions\": the steps it runs.".into());
    }
    if list.iter().any(|one| one.get("type").and_then(Value::as_str) == Some("function") && one.get("function").and_then(Value::as_str) == Some(id)) {
        out.push("It runs itself; that stops after 8 rounds.".into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn functions_go_in_place() {
        let greet = vec![json!({ "type": "message", "text": "hi" }), json!({ "type": "function", "function": "loop" })];
        let looping = vec![json!({ "type": "sound" }), json!({ "type": "function", "function": "loop" })];
        let lookup = |id: &str| match id {
            "greet" => Some(greet.clone()),
            "loop" => Some(looping.clone()),
            _ => None,
        };
        let steps = vec![json!({ "type": "title" }), json!({ "type": "function", "function": "greet" }), json!({ "type": "function", "function": "gone" }), json!({ "type": "heal" })];
        let out = expand(&steps, &lookup, 200);
        let types: Vec<&str> = out.iter().map(|one| one["type"].as_str().unwrap()).collect();
        // title, message, then "loop" 7 deep (sound each), then heal.
        assert_eq!(types.first(), Some(&"title"));
        assert_eq!(types[1], "message");
        assert_eq!(types.iter().filter(|one| **one == "sound").count(), MOST_DEPTH - 1);
        assert_eq!(types.last(), Some(&"heal"));
        assert_eq!(expand(&steps, &lookup, 2).len(), 2);
        let data: Data = serde_json::from_str(r#"{"actions":[{"type":"function","function":"me"}]}"#).unwrap();
        assert_eq!(problems("me", &data).len(), 1);
        assert_eq!(problems("x", &Data::new()).len(), 1);
    }
}
