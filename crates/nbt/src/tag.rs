//! The NBT value model. Compounds keep their keys in file order so a load and
//! save round trip leaves the file byte-identical apart from what was edited.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Kind {
    End = 0,
    Byte = 1,
    Short = 2,
    Int = 3,
    Long = 4,
    Float = 5,
    Double = 6,
    ByteArray = 7,
    String = 8,
    List = 9,
    Compound = 10,
    IntArray = 11,
    LongArray = 12,
}

impl Kind {
    pub fn from_id(id: u8) -> Option<Kind> {
        Some(match id {
            0 => Kind::End,
            1 => Kind::Byte,
            2 => Kind::Short,
            3 => Kind::Int,
            4 => Kind::Long,
            5 => Kind::Float,
            6 => Kind::Double,
            7 => Kind::ByteArray,
            8 => Kind::String,
            9 => Kind::List,
            10 => Kind::Compound,
            11 => Kind::IntArray,
            12 => Kind::LongArray,
            _ => return None,
        })
    }

    pub fn id(self) -> u8 {
        self as u8
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::End => "End",
            Kind::Byte => "Byte",
            Kind::Short => "Short",
            Kind::Int => "Int",
            Kind::Long => "Long",
            Kind::Float => "Float",
            Kind::Double => "Double",
            Kind::ByteArray => "Byte array",
            Kind::String => "String",
            Kind::List => "List",
            Kind::Compound => "Compound",
            Kind::IntArray => "Int array",
            Kind::LongArray => "Long array",
        }
    }

    /// Every kind a user can create, in the order an "add" menu lists them.
    pub const CREATABLE: [Kind; 12] = [
        Kind::Byte,
        Kind::Short,
        Kind::Int,
        Kind::Long,
        Kind::Float,
        Kind::Double,
        Kind::String,
        Kind::Compound,
        Kind::List,
        Kind::ByteArray,
        Kind::IntArray,
        Kind::LongArray,
    ];

    /// The empty or zero value of this kind.
    pub fn default_tag(self) -> Tag {
        match self {
            Kind::End | Kind::Byte => Tag::Byte(0),
            Kind::Short => Tag::Short(0),
            Kind::Int => Tag::Int(0),
            Kind::Long => Tag::Long(0),
            Kind::Float => Tag::Float(0.0),
            Kind::Double => Tag::Double(0.0),
            Kind::ByteArray => Tag::ByteArray(Vec::new()),
            Kind::String => Tag::String(String::new()),
            Kind::List => Tag::List(List::empty()),
            Kind::Compound => Tag::Compound(Compound::new()),
            Kind::IntArray => Tag::IntArray(Vec::new()),
            Kind::LongArray => Tag::LongArray(Vec::new()),
        }
    }

    pub fn is_container(self) -> bool {
        matches!(
            self,
            Kind::List | Kind::Compound | Kind::ByteArray | Kind::IntArray | Kind::LongArray
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(List),
    Compound(Compound),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    pub fn kind(&self) -> Kind {
        match self {
            Tag::Byte(_) => Kind::Byte,
            Tag::Short(_) => Kind::Short,
            Tag::Int(_) => Kind::Int,
            Tag::Long(_) => Kind::Long,
            Tag::Float(_) => Kind::Float,
            Tag::Double(_) => Kind::Double,
            Tag::ByteArray(_) => Kind::ByteArray,
            Tag::String(_) => Kind::String,
            Tag::List(_) => Kind::List,
            Tag::Compound(_) => Kind::Compound,
            Tag::IntArray(_) => Kind::IntArray,
            Tag::LongArray(_) => Kind::LongArray,
        }
    }

    /// How many children a container has; 0 for plain values.
    pub fn len(&self) -> usize {
        match self {
            Tag::ByteArray(v) => v.len(),
            Tag::IntArray(v) => v.len(),
            Tag::LongArray(v) => v.len(),
            Tag::List(list) => list.items.len(),
            Tag::Compound(map) => map.len(),
            _ => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn as_compound(&self) -> Option<&Compound> {
        match self {
            Tag::Compound(map) => Some(map),
            _ => None,
        }
    }

    pub fn as_compound_mut(&mut self) -> Option<&mut Compound> {
        match self {
            Tag::Compound(map) => Some(map),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        Some(match self {
            Tag::Byte(v) => *v as i64,
            Tag::Short(v) => *v as i64,
            Tag::Int(v) => *v as i64,
            Tag::Long(v) => *v,
            _ => return None,
        })
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Tag::String(text) => Some(text),
            _ => None,
        }
    }

    /// The child at one step of a path, if this is a container that has it.
    /// Array elements come back as owned tags, so they are not reachable here.
    pub fn child(&self, step: &Step) -> Option<&Tag> {
        match (self, step) {
            (Tag::Compound(map), Step::Key(key)) => map.get(key),
            (Tag::List(list), Step::Index(at)) => list.items.get(*at),
            _ => None,
        }
    }

    pub fn child_mut(&mut self, step: &Step) -> Option<&mut Tag> {
        match (self, step) {
            (Tag::Compound(map), Step::Key(key)) => map.get_mut(key),
            (Tag::List(list), Step::Index(at)) => list.items.get_mut(*at),
            _ => None,
        }
    }

    pub fn at(&self, path: &[Step]) -> Option<&Tag> {
        path.iter().try_fold(self, |tag, step| tag.child(step))
    }

    pub fn at_mut(&mut self, path: &[Step]) -> Option<&mut Tag> {
        path.iter().try_fold(self, |tag, step| tag.child_mut(step))
    }
}

/// One step into a container: a compound key or a list / array index.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Step {
    Key(String),
    Index(usize),
}

impl std::fmt::Display for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Step::Key(key) => f.write_str(key),
            Step::Index(at) => write!(f, "[{at}]"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct List {
    /// The element kind. Kept for empty lists, which still carry one on disk.
    pub kind: Kind,
    pub items: Vec<Tag>,
}

impl List {
    pub fn empty() -> List {
        List {
            kind: Kind::End,
            items: Vec::new(),
        }
    }

    pub fn of(items: Vec<Tag>) -> Result<List, String> {
        let kind = items.first().map(Tag::kind).unwrap_or(Kind::End);
        if items.iter().any(|one| one.kind() != kind) {
            return Err("Every item in a list must be the same type.".into());
        }
        Ok(List { kind, items })
    }

    /// Whether `tag` may join this list: empty lists take anything.
    pub fn accepts(&self, tag: &Tag) -> bool {
        self.items.is_empty() || self.kind == tag.kind()
    }

    pub fn push(&mut self, tag: Tag) -> Result<(), String> {
        self.insert(self.items.len(), tag)
    }

    pub fn insert(&mut self, at: usize, tag: Tag) -> Result<(), String> {
        if !self.accepts(&tag) {
            return Err(format!(
                "This list holds {} values, not {}.",
                self.kind.name(),
                tag.kind().name()
            ));
        }
        self.kind = tag.kind();
        self.items.insert(at.min(self.items.len()), tag);
        Ok(())
    }
}

/// A compound that keeps insertion order. Lookups are linear, which is faster
/// than hashing for the small compounds NBT is made of.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Compound(Vec<(String, Tag)>);

impl Compound {
    pub fn new() -> Compound {
        Compound(Vec::new())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Tag)> {
        self.0.iter().map(|(key, tag)| (key, tag))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&String, &mut Tag)> {
        self.0.iter_mut().map(|(key, tag)| (&*key, tag))
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.iter().map(|(key, _)| key)
    }

    pub fn position(&self, key: &str) -> Option<usize> {
        self.0.iter().position(|(one, _)| one == key)
    }

    pub fn get(&self, key: &str) -> Option<&Tag> {
        self.0
            .iter()
            .find(|(one, _)| one == key)
            .map(|(_, tag)| tag)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Tag> {
        self.0
            .iter_mut()
            .find(|(one, _)| one == key)
            .map(|(_, tag)| tag)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.position(key).is_some()
    }

    /// Sets `key`, keeping its place if it already exists. Returns the old value.
    pub fn insert(&mut self, key: impl Into<String>, tag: Tag) -> Option<Tag> {
        let key = key.into();
        match self.position(&key) {
            Some(at) => Some(std::mem::replace(&mut self.0[at].1, tag)),
            None => {
                self.0.push((key, tag));
                None
            }
        }
    }

    /// Inserts a new key at `at`, or replaces an existing key's value in place.
    pub fn insert_at(&mut self, at: usize, key: impl Into<String>, tag: Tag) {
        let key = key.into();
        match self.position(&key) {
            Some(found) => self.0[found].1 = tag,
            None => self.0.insert(at.min(self.0.len()), (key, tag)),
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<Tag> {
        let at = self.position(key)?;
        Some(self.0.remove(at).1)
    }

    /// Renames a key in place. Fails if the new name is taken.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        if from == to {
            return Ok(());
        }
        if self.contains(to) {
            return Err(format!("\"{to}\" already exists here."));
        }
        let at = self.position(from).ok_or("That key is gone.")?;
        self.0[at].0 = to.to_string();
        Ok(())
    }

    /// A key not used yet, starting from `base` ("name", "name 2", "name 3"…).
    pub fn free_key(&self, base: &str) -> String {
        if !self.contains(base) {
            return base.to_string();
        }
        (2..)
            .map(|n| format!("{base} {n}"))
            .find(|key| !self.contains(key))
            .expect("unbounded")
    }

    pub fn sort(&mut self) {
        self.0.sort_by(|a, b| a.0.cmp(&b.0));
    }
}

impl FromIterator<(String, Tag)> for Compound {
    fn from_iter<I: IntoIterator<Item = (String, Tag)>>(iter: I) -> Self {
        let mut out = Compound::new();
        for (key, tag) in iter {
            out.insert(key, tag);
        }
        out
    }
}
