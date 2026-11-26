//! The state behind an NBT editor view, kept free of any UI toolkit:
//! which nodes are open, the visible rows, edits with undo and redo, search.

use crate::binary::{Compression, NbtFile};
use crate::snbt;
use crate::tag::{Compound, Kind, Tag};
use std::collections::HashSet;

pub use crate::tag::Step;
pub type Path = Vec<Step>;

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub path: Path,
    pub depth: usize,
    /// The key, or `[n]` for list items. Empty for the root.
    pub label: String,
    pub kind: Kind,
    /// A one-line summary of the value.
    pub preview: String,
    pub children: usize,
    pub open: bool,
    /// Whether this row matched the search (ancestors are shown but dimmed).
    pub hit: bool,
}

impl Row {
    pub fn openable(&self) -> bool {
        matches!(self.kind, Kind::Compound | Kind::List) && self.children > 0
    }
}

#[derive(Clone, Debug)]
enum Op {
    /// The value at `path` changes from `before` to `after`.
    Set { path: Path, before: Tag, after: Tag },
    /// A child appears in the container at `parent`, at `at`, named `step`.
    Add {
        parent: Path,
        step: Step,
        at: usize,
        tag: Tag,
    },
    /// The reverse of `Add`.
    Remove {
        parent: Path,
        step: Step,
        at: usize,
        tag: Tag,
    },
    Rename {
        parent: Path,
        from: String,
        to: String,
    },
}

impl Op {
    fn inverse(&self) -> Op {
        match self.clone() {
            Op::Set {
                path,
                before,
                after,
            } => Op::Set {
                path,
                before: after,
                after: before,
            },
            Op::Add {
                parent,
                step,
                at,
                tag,
            } => Op::Remove {
                parent,
                step,
                at,
                tag,
            },
            Op::Remove {
                parent,
                step,
                at,
                tag,
            } => Op::Add {
                parent,
                step,
                at,
                tag,
            },
            Op::Rename { parent, from, to } => Op::Rename {
                parent,
                from: to,
                to: from,
            },
        }
    }
}

pub struct Editor {
    pub name: String,
    pub compression: Compression,
    root: Tag,
    open: HashSet<Path>,
    undo: Vec<Op>,
    redo: Vec<Op>,
    /// `undo.len()` at the last save; `None` once that point is unreachable.
    saved: Option<usize>,
}

fn container_len(tag: &Tag) -> usize {
    match tag {
        Tag::Compound(_) | Tag::List(_) => tag.len(),
        _ => 0,
    }
}

/// A short summary of a value for its row.
pub fn preview(tag: &Tag) -> String {
    fn count(n: usize, one: &str, many: &str) -> String {
        format!("{n} {}", if n == 1 { one } else { many })
    }
    match tag {
        Tag::Compound(map) => count(map.len(), "entry", "entries"),
        Tag::List(list) => {
            if list.items.is_empty() {
                "empty".into()
            } else {
                format!(
                    "{} · {}",
                    count(list.items.len(), "item", "items"),
                    list.kind.name()
                )
            }
        }
        Tag::ByteArray(_) | Tag::IntArray(_) | Tag::LongArray(_) => {
            let text = snbt::to_snbt(tag, false);
            if text.len() > 96 {
                let cut = text
                    .char_indices()
                    .nth(90)
                    .map(|(i, _)| i)
                    .unwrap_or(text.len());
                format!(
                    "{}… ({})",
                    &text[..cut],
                    count(tag.len(), "value", "values")
                )
            } else {
                text
            }
        }
        Tag::String(text) => {
            let line = text.replace('\n', "⏎");
            if line.chars().count() > 120 {
                format!("{}…", line.chars().take(117).collect::<String>())
            } else {
                line
            }
        }
        other => snbt::edit_text(other),
    }
}

fn label(step: Option<&Step>) -> String {
    match step {
        None => String::new(),
        Some(Step::Key(key)) => key.clone(),
        Some(Step::Index(at)) => format!("[{at}]"),
    }
}

impl Editor {
    pub fn new(file: NbtFile) -> Editor {
        let mut open = HashSet::new();
        open.insert(Vec::new());
        Editor {
            name: file.name,
            compression: file.compression,
            root: Tag::Compound(file.root),
            open,
            undo: Vec::new(),
            redo: Vec::new(),
            saved: Some(0),
        }
    }

    pub fn root(&self) -> &Compound {
        self.root.as_compound().expect("root is a compound")
    }

    pub fn get(&self, path: &[Step]) -> Option<&Tag> {
        self.root.at(path)
    }

    /// The document as it should be written back.
    pub fn file(&self) -> NbtFile {
        NbtFile {
            name: self.name.clone(),
            root: self.root().clone(),
            compression: self.compression,
        }
    }

    pub fn dirty(&self) -> bool {
        self.saved != Some(self.undo.len())
    }

    pub fn mark_saved(&mut self) {
        self.saved = Some(self.undo.len());
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn is_open(&self, path: &[Step]) -> bool {
        self.open.contains(path)
    }

    pub fn toggle(&mut self, path: &[Step]) {
        if !self.open.remove(path) {
            self.open.insert(path.to_vec());
        }
    }

    pub fn set_open(&mut self, path: &[Step], open: bool) {
        if open {
            self.open.insert(path.to_vec());
        } else {
            self.open.remove(path);
        }
    }

    /// Opens every container under `path` (the whole tree for the root).
    pub fn expand_all(&mut self, path: &[Step]) {
        let mut stack = vec![path.to_vec()];
        while let Some(at) = stack.pop() {
            let Some(tag) = self.root.at(&at) else {
                continue;
            };
            if container_len(tag) == 0 {
                continue;
            }
            for step in child_steps(tag) {
                let mut next = at.clone();
                next.push(step);
                stack.push(next);
            }
            self.open.insert(at);
        }
    }

    pub fn collapse_all(&mut self) {
        self.open.clear();
        self.open.insert(Vec::new());
    }

    /// The rows to draw, in order. With a search, only matching rows and the
    /// containers that lead to them, all shown open.
    pub fn rows(&self, search: &str) -> Vec<Row> {
        let needle = search.trim().to_lowercase();
        let mut out = Vec::new();
        if needle.is_empty() {
            self.walk(&self.root, &mut Vec::new(), None, 0, &mut out);
            return out;
        }
        let mut path = Vec::new();
        search_walk(&self.root, &mut path, None, 0, &needle, &mut out);
        out
    }

    fn walk(
        &self,
        tag: &Tag,
        path: &mut Path,
        step: Option<&Step>,
        depth: usize,
        out: &mut Vec<Row>,
    ) {
        let open = self.open.contains(path.as_slice());
        out.push(Row {
            path: path.clone(),
            depth,
            label: label(step),
            kind: tag.kind(),
            preview: preview(tag),
            children: container_len(tag),
            open,
            hit: false,
        });
        if !open {
            return;
        }
        match tag {
            Tag::Compound(map) => {
                for (key, child) in map.iter() {
                    let step = Step::Key(key.clone());
                    path.push(step.clone());
                    self.walk(child, path, Some(&step), depth + 1, out);
                    path.pop();
                }
            }
            Tag::List(list) => {
                for (at, child) in list.items.iter().enumerate() {
                    let step = Step::Index(at);
                    path.push(step.clone());
                    self.walk(child, path, Some(&step), depth + 1, out);
                    path.pop();
                }
            }
            _ => {}
        }
    }

    fn apply(&mut self, op: &Op) -> Result<(), String> {
        let gone = || "That value no longer exists.".to_string();
        match op {
            Op::Set { path, after, .. } => {
                *self.root.at_mut(path).ok_or_else(gone)? = after.clone();
            }
            Op::Add {
                parent,
                step,
                at,
                tag,
            } => match (self.root.at_mut(parent).ok_or_else(gone)?, step) {
                (Tag::Compound(map), Step::Key(key)) => {
                    if map.contains(key) {
                        return Err(format!("\"{key}\" already exists here."));
                    }
                    map.insert_at(*at, key.clone(), tag.clone());
                }
                (Tag::List(list), Step::Index(_)) => list.insert(*at, tag.clone())?,
                _ => return Err("Values can only be added to a compound or a list.".into()),
            },
            Op::Remove { parent, step, .. } => {
                match (self.root.at_mut(parent).ok_or_else(gone)?, step) {
                    (Tag::Compound(map), Step::Key(key)) => {
                        map.remove(key).ok_or_else(gone)?;
                    }
                    (Tag::List(list), Step::Index(at)) => {
                        if *at >= list.items.len() {
                            return Err(gone());
                        }
                        list.items.remove(*at);
                    }
                    _ => return Err(gone()),
                }
            }
            Op::Rename { parent, from, to } => {
                let map = self
                    .root
                    .at_mut(parent)
                    .and_then(Tag::as_compound_mut)
                    .ok_or_else(gone)?;
                map.rename(from, to)?;
            }
        }
        self.fix_open(op);
        Ok(())
    }

    /// Keeps open/closed state attached to the right nodes after a change.
    fn fix_open(&mut self, op: &Op) {
        let shift = |paths: &HashSet<Path>, parent: &Path, f: &dyn Fn(&Step) -> Option<Step>| {
            paths
                .iter()
                .filter_map(|path| {
                    if path.len() > parent.len() && path.starts_with(parent) {
                        let mut next = path.clone();
                        next[parent.len()] = f(&path[parent.len()])?;
                        Some(next)
                    } else {
                        Some(path.clone())
                    }
                })
                .collect::<HashSet<_>>()
        };
        self.open = match op {
            Op::Rename { parent, from, to } => shift(&self.open, parent, &|step| {
                Some(match step {
                    Step::Key(key) if key == from => Step::Key(to.clone()),
                    other => other.clone(),
                })
            }),
            Op::Add {
                parent,
                step: Step::Index(_),
                at,
                ..
            } => shift(&self.open, parent, &|step| {
                Some(match step {
                    Step::Index(i) if i >= at => Step::Index(i + 1),
                    other => other.clone(),
                })
            }),
            Op::Remove { parent, step, .. } => {
                shift(&self.open, parent, &|one| match (one, step) {
                    (Step::Index(i), Step::Index(at)) if i == at => None,
                    (Step::Index(i), Step::Index(at)) if i > at => Some(Step::Index(i - 1)),
                    (Step::Key(a), Step::Key(b)) if a == b => None,
                    (other, _) => Some(other.clone()),
                })
            }
            Op::Set {
                path,
                before,
                after,
            } if before.kind() != after.kind() => self
                .open
                .iter()
                .filter(|one| !(one.len() > path.len() && one.starts_with(path)))
                .cloned()
                .collect(),
            _ => return,
        };
    }

    fn run(&mut self, op: Op) -> Result<(), String> {
        self.apply(&op)?;
        if self.saved.is_some_and(|at| at > self.undo.len()) {
            self.saved = None;
        }
        self.undo.push(op);
        self.redo.clear();
        Ok(())
    }

    pub fn undo(&mut self) -> Result<(), String> {
        let Some(op) = self.undo.pop() else {
            return Ok(());
        };
        self.apply(&op.inverse())?;
        self.redo.push(op);
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), String> {
        let Some(op) = self.redo.pop() else {
            return Ok(());
        };
        self.apply(&op)?;
        self.undo.push(op);
        Ok(())
    }

    /// Replaces a value with what the user typed, read as the same type.
    pub fn set_text(&mut self, path: &[Step], text: &str) -> Result<(), String> {
        let before = self
            .get(path)
            .ok_or("That value no longer exists.")?
            .clone();
        let after = snbt::parse_as(before.kind(), text)?;
        self.set(path, after)
    }

    pub fn set(&mut self, path: &[Step], after: Tag) -> Result<(), String> {
        if path.is_empty() && after.kind() != Kind::Compound {
            return Err("The root must stay a compound.".into());
        }
        let before = self
            .get(path)
            .ok_or("That value no longer exists.")?
            .clone();
        if before == after {
            return Ok(());
        }
        if let (Some((Step::Index(_), parent)), true) =
            (path.split_last(), before.kind() != after.kind())
        {
            if let Some(Tag::List(list)) = self.get(parent) {
                if list.items.len() > 1 {
                    return Err(format!(
                        "Every item in this list must be a {}.",
                        list.kind.name()
                    ));
                }
            }
        }
        self.run(Op::Set {
            path: path.to_vec(),
            before,
            after,
        })
    }

    pub fn rename(&mut self, path: &[Step], to: &str) -> Result<(), String> {
        let Some((Step::Key(from), parent)) = path.split_last() else {
            return Err("Only compound entries have names.".into());
        };
        if from == to {
            return Ok(());
        }
        if self
            .get(parent)
            .and_then(Tag::as_compound)
            .is_some_and(|map| map.contains(to))
        {
            return Err(format!("\"{to}\" already exists here."));
        }
        self.run(Op::Rename {
            parent: parent.to_vec(),
            from: from.clone(),
            to: to.to_string(),
        })
    }

    pub fn remove(&mut self, path: &[Step]) -> Result<(), String> {
        let Some((step, parent)) = path.split_last() else {
            return Err("The root cannot be removed.".into());
        };
        let tag = self
            .get(path)
            .ok_or("That value no longer exists.")?
            .clone();
        let at = match (self.get(parent), step) {
            (Some(Tag::Compound(map)), Step::Key(key)) => map.position(key).unwrap_or(0),
            (_, Step::Index(at)) => *at,
            _ => 0,
        };
        self.run(Op::Remove {
            parent: parent.to_vec(),
            step: step.clone(),
            at,
            tag,
        })
    }

    /// Adds `tag` inside the container at `parent` (or after `parent` when it
    /// is not a container) and returns the new value's path.
    pub fn add(&mut self, target: &[Step], tag: Tag, name: &str) -> Result<Path, String> {
        let into_target = matches!(self.get(target), Some(Tag::Compound(_) | Tag::List(_)));
        let (parent, at) = if into_target {
            (target.to_vec(), usize::MAX)
        } else {
            let (last, parent) = target.split_last().ok_or("Nothing to add to.")?;
            let at = match (self.get(parent), last) {
                (Some(Tag::Compound(map)), Step::Key(key)) => {
                    map.position(key).map_or(usize::MAX, |at| at + 1)
                }
                (_, Step::Index(at)) => at + 1,
                _ => usize::MAX,
            };
            (parent.to_vec(), at)
        };
        let step = match self.get(&parent) {
            Some(Tag::Compound(map)) => {
                Step::Key(map.free_key(if name.is_empty() { "new" } else { name }))
            }
            Some(Tag::List(list)) => {
                if !list.accepts(&tag) {
                    return Err(format!("This list holds {} values.", list.kind.name()));
                }
                Step::Index(at.min(list.items.len()))
            }
            _ => return Err("Values can only be added to a compound or a list.".into()),
        };
        let at = match &step {
            Step::Index(i) => *i,
            Step::Key(_) => at,
        };
        self.run(Op::Add {
            parent: parent.clone(),
            step: step.clone(),
            at,
            tag,
        })?;
        self.open.insert(parent.clone());
        let mut path = parent;
        path.push(step);
        Ok(path)
    }

    /// What a new child of `target` should be: a list's own item type, else `Int`.
    pub fn suggested_kind(&self, target: &[Step]) -> Kind {
        match self.get(target) {
            Some(Tag::List(list)) if !list.items.is_empty() => list.kind,
            _ => Kind::Int,
        }
    }

    pub fn copy_snbt(&self, path: &[Step]) -> Option<String> {
        self.get(path).map(|tag| snbt::to_snbt(tag, false))
    }
}

fn child_steps(tag: &Tag) -> Vec<Step> {
    match tag {
        Tag::Compound(map) => map.keys().map(|key| Step::Key(key.clone())).collect(),
        Tag::List(list) => (0..list.items.len()).map(Step::Index).collect(),
        _ => Vec::new(),
    }
}

/// Pushes rows for every match under `tag`; returns whether anything matched.
fn search_walk(
    tag: &Tag,
    path: &mut Path,
    step: Option<&Step>,
    depth: usize,
    needle: &str,
    out: &mut Vec<Row>,
) -> bool {
    let name = label(step);
    let hit = !path.is_empty()
        && (name.to_lowercase().contains(needle)
            || (!matches!(tag, Tag::Compound(_) | Tag::List(_))
                && preview(tag).to_lowercase().contains(needle)));
    let mark = out.len();
    out.push(Row {
        path: path.clone(),
        depth,
        label: name,
        kind: tag.kind(),
        preview: preview(tag),
        children: container_len(tag),
        open: true,
        hit,
    });
    let mut any = false;
    for child_step in child_steps(tag) {
        let child = tag.child(&child_step).expect("listed child");
        path.push(child_step.clone());
        any |= search_walk(child, path, Some(&child_step), depth + 1, needle, out);
        path.pop();
    }
    if !hit && !any && depth > 0 {
        out.truncate(mark);
        return false;
    }
    if !any {
        out[mark].open = false;
    }
    hit || any
}
