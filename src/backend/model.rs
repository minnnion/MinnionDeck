//! What the window shows and edits: the profiles, and the pages and buttons of the profile in
//! use. backend/library.rs reads it from library.json and writes it back.

/// A page or folder in the pages panel, flattened: `depth` is how deep in folders it is.
#[derive(Clone, Hash)]
pub struct PageRow {
    /// Its id in library.json ("" for one made in this session; library.rs gives it one).
    pub id: String,
    /// The board (list of buttons) of a page.
    pub board: String,
    pub name: String,
    pub folder: bool,
    pub depth: usize,
    pub open: bool,
    pub count: usize,
    /// A page's grid size.
    pub cols: usize,
    pub rows: usize,
}

impl PageRow {
    pub fn new_page(name: &str) -> PageRow {
        PageRow { id: String::new(), board: String::new(), name: name.into(), folder: false, depth: 0, open: true, count: 0, cols: 5, rows: 3 }
    }
    pub fn new_folder(name: &str) -> PageRow {
        PageRow { folder: true, ..PageRow::new_page(name) }
    }
}

/// A picture or GIF on one look of a button (backend/media.rs). Empty: just the colour.
#[derive(Clone, Default, Hash, PartialEq, Debug)]
pub struct Media {
    /// A picture's id ("u:…"): fills the button behind the icon and the name
    pub pic: String,
    /// "Show it all" instead of "Fill the button"
    pub contain: bool,
    /// A GIF's id: plays behind everything, or in the icon's place
    pub gif: String,
    pub gif_icon: bool,
    /// 0 slow, 1 normal, 2 fast
    pub speed: u8,
}

impl Media {
    pub fn is_empty(&self) -> bool {
        self.pic.is_empty() && self.gif.is_empty()
    }
}

/// One look of a button (a state). The first look is the button's own fields; each press moves
/// to the next look.
#[derive(Clone, Default, Hash)]
pub struct Look {
    pub name: String,
    /// Text on the button; empty uses the button's name.
    pub label: String,
    pub icon: String,
    pub colour: String,
    pub media: Media,
}

/// What a plugin is showing on a button right now: a look it switched to, a text and a picture.
/// Live only: never saved, and it doesn't count as a change (so it doesn't hash).
#[derive(Clone, Default, PartialEq)]
pub struct Live {
    pub state: Option<usize>,
    pub title: Option<String>,
    /// "png:live/<plugin>/<file>", registered in ui::images
    pub icon: Option<String>,
}

impl std::hash::Hash for Live {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

#[derive(Clone, Hash)]
pub struct Tile {
    /// Its id in library.json ("" for a new one), and for a copy the button it was copied
    /// from, so the copy keeps that button's design and settings.
    pub id: String,
    pub src: String,
    /// The page (index into the page list) the button is on.
    pub page: usize,
    pub col: usize,
    pub row: usize,
    pub w: usize,
    pub h: usize,
    pub label: String,
    pub icon: String,
    /// Accent name from the theme ("sky", "lilac", ...), "tile" for the plain fill, or "#rrggbb".
    pub colour: String,
    /// An app logo or picture chosen in the editor ("px:..." / "si:..." / "png:..."), drawn instead of the icon.
    pub image: String,
    pub anim: usize,
    /// The action id ("key", "plug:minnion_spotify:next", ...) and its field values.
    pub action: String,
    pub values: Vec<(String, String)>,
    /// The name of the first look, the looks after it, and the one showing now.
    pub first_name: String,
    pub more: Vec<Look>,
    pub cur: usize,
    pub live: Live,
    /// The first look's picture and GIF.
    pub media: Media,
}

impl Tile {
    pub fn new(page: usize, col: usize, row: usize) -> Tile {
        Tile {
            id: String::new(),
            src: String::new(),
            page,
            col,
            row,
            w: 1,
            h: 1,
            label: String::new(),
            icon: String::new(),
            colour: String::new(),
            image: String::new(),
            anim: 0,
            action: String::new(),
            values: vec![],
            first_name: String::new(),
            more: vec![],
            cur: 0,
            live: Live::default(),
            media: Media::default(),
        }
    }

    pub fn looks(&self) -> usize {
        1 + self.more.len()
    }

    /// The look showing now: the one a plugin switched to, or the one the button is on.
    pub fn look_now(&self) -> usize {
        self.live.state.map(|s| s.min(self.more.len())).unwrap_or(self.cur)
    }

    /// (text, picture, colour) of the look showing now, with what a plugin put on it.
    pub fn shown(&self) -> (&str, &str, &str) {
        let base_icon = if self.image.is_empty() { self.icon.as_str() } else { self.image.as_str() };
        let (label, icon, colour) = match self.look_now().checked_sub(1).and_then(|i| self.more.get(i)) {
            Some(l) => (if l.label.is_empty() { self.label.as_str() } else { l.label.as_str() }, l.icon.as_str(), l.colour.as_str()),
            None => (self.label.as_str(), base_icon, self.colour.as_str()),
        };
        (self.live.title.as_deref().unwrap_or(label), self.live.icon.as_deref().unwrap_or(icon), colour)
    }

    /// The picture and GIF of the look showing now.
    pub fn media_now(&self) -> &Media {
        match self.look_now().checked_sub(1).and_then(|i| self.more.get(i)) {
            Some(l) => &l.media,
            None => &self.media,
        }
    }

    pub fn value(&self, key: &str) -> &str {
        self.values.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).unwrap_or("")
    }
}

#[derive(Clone, Hash)]
pub struct Profile {
    /// Its id in library.json ("" for a new one).
    pub id: String,
    pub name: String,
    pub colour: String,
    pub icon: String,
    /// Index into AUTO_CHOICES.
    pub auto: usize,
    pub pages: usize,
    pub folders: usize,
    pub buttons: usize,
}

/// A paired phone or tablet.
#[derive(Clone)]
pub struct Phone {
    /// Its id in the phone page's list of paired phones.
    pub id: String,
    pub name: String,
    pub model: String,
    /// Connected now; otherwise when it was last seen ("2 days ago").
    pub online: bool,
    pub seen: String,
    pub tablet: bool,
}

pub struct Sample {
    pub phones: Vec<Phone>,
    pub profiles: Vec<Profile>,
    pub active: usize,
    pub pages: Vec<PageRow>,
    pub page: usize,
    pub tiles: Vec<Tile>,
    /// The grid size of the page shown (library.rs keeps it in step with the page).
    pub cols: usize,
    pub rows: usize,
}

pub const COLOURS: [(&str, &str); 6] =
    [("mint", "Mint"), ("sky", "Sky"), ("lilac", "Lilac"), ("peach", "Peach"), ("coral", "Coral"), ("sun", "Sun")];

/// When a profile switches on by itself, as the Minniondeck app offers it (its "auto" field is
/// the app's name, or "Never").
pub const AUTO_CHOICES: [&str; 7] = [
    "Never (switch by hand)",
    "OBS Studio is in front",
    "Microsoft Teams is in front",
    "Steam is in front",
    "Spotify is in front",
    "Visual Studio Code is in front",
    "Any full-screen game is in front",
];

/// "Never" or the app's name, as library.json keeps it.
pub fn auto_name(i: usize) -> &'static str {
    match i {
        0 => "Never",
        _ => AUTO_CHOICES.get(i).map(|c| c.trim_end_matches(" is in front")).unwrap_or("Never"),
    }
}

pub fn auto_index(name: &str) -> usize {
    (1..AUTO_CHOICES.len()).find(|&i| auto_name(i) == name).unwrap_or(0)
}

impl Sample {
    /// Nothing loaded yet: one profile with one empty page.
    pub fn empty() -> Sample {
        Sample {
            phones: vec![],
            profiles: vec![Profile { id: String::new(), name: "My profile".into(), colour: "sky".into(), icon: "star".into(), auto: 0, pages: 1, folders: 0, buttons: 0 }],
            active: 0,
            pages: vec![PageRow::new_page("Page 1")],
            page: 0,
            tiles: vec![],
            cols: 5,
            rows: 3,
        }
    }
}
