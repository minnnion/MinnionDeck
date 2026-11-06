//! The list of actions the Edit button window offers, kept up to date from the plugins that are
//! installed: each plugin's actions.json and the pictures its manifest names.
//!
//! `src/generated/catalog.rs` (made by tools/gen_catalog.cjs) is the starting point: the built-in
//! actions, and the actions of the plugins that were on the developer's PC when it was made. A
//! plugin that is installed here replaces its entry there with what its own folder says, and a
//! plugin that isn't in the generated list at all (one published to the marketplace later) is
//! added. Actions of a plugin that is not installed stay listed, so a saved button that uses it
//! still opens in the editor and keeps its action.
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::RwLock;

use serde_json::Value;

use super::plugins::{check_manifest, read_icon, safe_name, safe_rel};
use crate::generated::catalog as gen_catalog;
use crate::generated::catalog::{Action, Field, Plugin};

struct Live {
    actions: &'static [Action],
    plugins: &'static [Plugin],
    icons: &'static [(&'static str, &'static str)],
}

static LIVE: RwLock<Option<Live>> = RwLock::new(None);

fn live<T>(f: impl Fn(&Live) -> T, fallback: T) -> T {
    LIVE.read().ok().and_then(|l| l.as_ref().map(f)).unwrap_or(fallback)
}

/// Every action the editor offers.
pub fn actions() -> &'static [Action] {
    live(|l| l.actions, gen_catalog::ACTIONS)
}

/// The plugins that have actions, in the order the editor lists them.
pub fn plugins() -> &'static [Plugin] {
    live(|l| l.plugins, gen_catalog::PLUGINS)
}

/// The pictures plugins offer for buttons: (plugin name, picture key).
pub fn plugin_icons() -> &'static [(&'static str, &'static str)] {
    live(|l| l.icons, gen_catalog::PLUGIN_ICONS)
}

/// Whether a plugin picture ("plugin/name") can be drawn: built in, or read from the plugin.
pub fn has_picture(key: &str) -> bool {
    gen_catalog::png(key).is_some() || crate::ui::images::has_live(key)
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

fn leak_all(v: Vec<String>) -> &'static [&'static str] {
    Box::leak(v.iter().map(|s| leak(s)).collect::<Vec<_>>().into_boxed_slice())
}

fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// One plugin as the editor knows it: its name, the pictures it offers, its actions.
struct Found {
    id: String,
    name: String,
    icons: Vec<String>,
    actions: Vec<Action>,
}

/// A picture of a plugin: "png:<plugin>/<name>" (built in, or read from its folder and kept for
/// drawing), or the plain bolt when it has none.
fn picture(folder: &Path, id: &str, file: &str) -> String {
    if !safe_rel(file) {
        return "bolt".into();
    }
    let name = Path::new(file).file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if !safe_name(name) {
        return "bolt".into();
    }
    let key = format!("{id}/{name}");
    if gen_catalog::png(&key).is_none() {
        match read_icon(folder, file) {
            Some(bytes) => crate::ui::images::register_live(&key, bytes),
            None => return "bolt".into(),
        }
    }
    format!("png:{key}")
}

fn read_plugin(folder: &Path) -> Option<Found> {
    let m = check_manifest(&fs::read_to_string(folder.join("manifest.json")).ok()?).ok()?;
    if folder.file_name()?.to_str()? != m.id {
        return None;
    }
    let manifest: Value = serde_json::from_str(&fs::read_to_string(folder.join("manifest.json")).ok()?).ok()?;
    let defs: Vec<Value> = serde_json::from_str(&fs::read_to_string(folder.join(&m.actions)).ok()?).ok()?;
    let mut actions = vec![];
    for a in defs.iter().filter(|a| a.get("id").and_then(|i| i.as_str()).is_some_and(safe_name)) {
        let aid = text(a.get("id"));
        let fields: Vec<Field> = a
            .get("fields")
            .and_then(|f| f.as_array())
            .into_iter()
            .flatten()
            .filter(|f| f.get("key").and_then(|k| k.as_str()).is_some_and(|k| !k.is_empty()))
            .map(|f| {
                let options: Vec<String> = f.get("options").and_then(|o| o.as_array()).map(|o| o.iter().map(|v| text(Some(v))).collect()).unwrap_or_default();
                let kind = match (f.get("kind").and_then(|k| k.as_str()), f.get("dynamic").and_then(|d| d.as_bool()).unwrap_or(false)) {
                    (Some("select"), true) => "dynamic",
                    (Some("select"), false) if options.is_empty() => "text",
                    (Some("select"), false) => "select",
                    (Some("number"), _) => "number",
                    (Some("url"), _) => "url",
                    _ => "text",
                };
                let key = text(f.get("key"));
                Field {
                    key: leak(&key),
                    label: leak(&match text(f.get("label")) { l if l.is_empty() => key.clone(), l => l }),
                    kind,
                    options: leak_all(options),
                    def: leak(&text(f.get("def"))),
                    placeholder: leak(&text(f.get("placeholder"))),
                    help: leak(&text(f.get("help"))),
                    unit: leak(&text(f.get("unit"))),
                    required: f.get("required").and_then(|r| r.as_bool()).unwrap_or(false),
                }
            })
            .collect();
        let states: Vec<String> = a.get("states").and_then(|s| s.as_array()).map(|s| s.iter().take(16).map(|v| text(Some(v)).chars().take(24).collect()).collect()).unwrap_or_default();
        let icon = picture(folder, &m.id, &text(a.get("icon")));
        let label = match text(a.get("label")) { l if l.is_empty() => aid.clone(), l => l };
        let group = match text(a.get("group")) { g if g.is_empty() => m.name.clone(), g => g };
        actions.push(Action {
            id: leak(&format!("plug:{}:{aid}", m.id)),
            label: leak(&label),
            group: leak(&group),
            plugin: leak(&m.id),
            plugin_name: leak(&m.name),
            icon: leak(&icon),
            help: leak(&text(a.get("help"))),
            states: leak_all(states),
            knob: a.get("knob").and_then(|k| k.as_bool()).unwrap_or(false),
            screen: a.get("screen").and_then(|k| k.as_bool()).unwrap_or(false),
            fields: Box::leak(fields.into_boxed_slice()),
        });
    }
    let icons = manifest.get("icons").and_then(|i| i.as_array()).into_iter().flatten().filter_map(|f| f.as_str()).map(|f| picture(folder, &m.id, f)).filter(|p| p.starts_with("png:")).map(|p| p[4..].to_string()).collect();
    Some(Found { id: m.id, name: m.name, icons, actions })
}

/// Reads the installed plugins in <root>\plugins again and makes the list from them. Call it on
/// the window's thread (the pictures are registered for drawing).
pub fn rebuild(root: &Path) {
    let dir = root.join("plugins");
    let mut found: Vec<Found> = fs::read_dir(&dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir() && !p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.'))).filter_map(|p| read_plugin(&p)).collect())
        .unwrap_or_default();
    found.sort_by(|a, b| a.id.cmp(&b.id));
    let installed: HashSet<String> = found.iter().map(|f| f.id.clone()).collect();

    // the generated list in its order; an installed plugin takes its own entry from its folder
    let mut actions: Vec<Action> = vec![];
    let mut plugins: Vec<Plugin> = vec![];
    let mut icons: Vec<(&'static str, &'static str)> = vec![];
    let mut order: Vec<&str> = gen_catalog::PLUGINS.iter().map(|p| p.id).collect();
    for f in &found {
        if !order.contains(&f.id.as_str()) {
            order.push(leak(&f.id));
        }
    }
    let copy = |a: &Action| Action { id: a.id, label: a.label, group: a.group, plugin: a.plugin, plugin_name: a.plugin_name, icon: a.icon, help: a.help, states: a.states, knob: a.knob, screen: a.screen, fields: a.fields };
    actions.extend(gen_catalog::ACTIONS.iter().filter(|a| a.plugin.is_empty()).map(copy));
    for id in order {
        match found.iter_mut().find(|f| f.id == id) {
            Some(f) => {
                let mine = std::mem::take(&mut f.actions);
                plugins.push(Plugin { id: leak(&f.id), name: leak(&f.name), icon: mine.first().map_or("bolt", |a| a.icon), count: mine.len() });
                actions.extend(mine);
                icons.extend(f.icons.iter().map(|k| (leak(&f.name), leak(k))));
            }
            None if !installed.contains(id) => {
                // not installed: what the generated list says stays, so saved buttons keep their action
                if let Some(p) = gen_catalog::PLUGINS.iter().find(|p| p.id == id) {
                    plugins.push(Plugin { id: p.id, name: p.name, icon: p.icon, count: p.count });
                    actions.extend(gen_catalog::ACTIONS.iter().filter(|a| a.plugin == id).map(copy));
                    icons.extend(gen_catalog::PLUGIN_ICONS.iter().filter(|(n, _)| *n == p.name).copied());
                }
            }
            None => {}
        }
    }
    let new = Live { actions: Box::leak(actions.into_boxed_slice()), plugins: Box::leak(plugins.into_boxed_slice()), icons: Box::leak(icons.into_boxed_slice()) };
    if let Ok(mut l) = LIVE.write() {
        *l = Some(new);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_plugin_adds_its_actions_and_an_installed_one_replaces_its_entry() {
        let root = std::env::temp_dir().join(format!("md-catalog-test-{}", super::super::util::now_ms()));
        let make = |id: &str, name: &str, actions: &str| {
            let dir = root.join("plugins").join(id);
            fs::create_dir_all(dir.join("icons")).unwrap();
            fs::write(dir.join("manifest.json"), format!(r#"{{"id":"{id}","name":"{name}","version":"1.0.0","api":2,"dll":"x.dll","icons":["icons/a.png","icons/missing.png"]}}"#)).unwrap();
            fs::write(dir.join("actions.json"), actions).unwrap();
            fs::write(dir.join("icons").join("a.png"), b"png-bytes").unwrap();
        };
        // a plugin the generated list never heard of
        make(
            "demo_new",
            "Demo",
            r#"[{"id":"go","label":"Go","icon":"icons/a.png","group":"Demo group","states":["Off","On"],"knob":true,
                 "fields":[{"key":"n","label":"Number","kind":"number","def":5,"unit":"s","required":true},
                           {"key":"drive","kind":"select","dynamic":true},
                           {"key":"pick","kind":"select","options":["A","B"]},
                           {"key":"empty","kind":"select"},
                           {"label":"no key"}]},
                {"id":"../bad","label":"Bad"}]"#,
        );
        // an installed plugin the generated list knows: it takes the folder's actions
        make("minnion_timer", "Timer", r#"[{"id":"only","label":"The only one"}]"#);
        rebuild(&root);
        let acts = actions();
        let go = acts.iter().find(|a| a.id == "plug:demo_new:go").expect("the new plugin's action");
        assert_eq!((go.label, go.group, go.plugin, go.plugin_name), ("Go", "Demo group", "demo_new", "Demo"));
        assert_eq!(go.states, &["Off", "On"]);
        assert!(go.knob && !go.screen);
        assert_eq!(go.icon, "png:demo_new/a", "a picture that isn't built in is read from the folder");
        assert!(has_picture("demo_new/a"));
        let kinds: Vec<(&str, &str, &str)> = go.fields.iter().map(|f| (f.key, f.kind, f.def)).collect();
        assert_eq!(kinds, [("n", "number", "5"), ("drive", "dynamic", ""), ("pick", "select", ""), ("empty", "text", "")]);
        assert!(!acts.iter().any(|a| a.id.contains("bad")), "an unsafe action id is left out");
        assert!(plugins().iter().any(|p| p.id == "demo_new" && p.count == 1));
        assert!(plugin_icons().iter().any(|(n, k)| *n == "Demo" && *k == "demo_new/a"));
        assert!(!plugin_icons().iter().any(|(_, k)| k.contains("missing")));
        // the timer was replaced by what its folder says, the built-ins stayed
        assert!(acts.iter().any(|a| a.id == "plug:minnion_timer:only"));
        assert!(!acts.iter().any(|a| a.id == "plug:minnion_timer:countdown"));
        assert!(acts.iter().any(|a| a.id == "key"));
        // a plugin that is not installed keeps its generated actions
        assert!(acts.iter().any(|a| a.plugin == "minnion_spotify"), "uninstalled plugins stay listed");
        let _ = fs::remove_dir_all(&root);
    }
}

/// Reads the plugins installed in build\plugins and compares what they say with the generated
/// list (made from the same plugins): the editor must offer the same actions and fields.
///     cargo test --release real_plugins_match -- --ignored --nocapture
#[cfg(test)]
mod real_plugins {
    use super::*;

    #[test]
    #[ignore]
    fn real_plugins_match_the_generated_list() {
        let root = super::super::paths::Paths::find().root;
        rebuild(&root);
        let mut checked = 0;
        for p in gen_catalog::PLUGINS {
            if !root.join("plugins").join(p.id).join("manifest.json").is_file() {
                println!("{}: not installed here, generated entries kept", p.id);
                continue;
            }
            let (live_a, gen_a): (Vec<&Action>, Vec<&Action>) = (actions().iter().filter(|a| a.plugin == p.id).collect(), gen_catalog::ACTIONS.iter().filter(|a| a.plugin == p.id).collect());
            println!("{}: {} actions live, {} generated", p.id, live_a.len(), gen_a.len());
            assert_eq!(live_a.len(), gen_a.len(), "{} has a different number of actions", p.id);
            for g in gen_a {
                let l = live_a.iter().find(|a| a.id == g.id).unwrap_or_else(|| panic!("{} is missing", g.id));
                assert_eq!((l.label, l.group, l.knob, l.screen, l.states), (g.label, g.group, g.knob, g.screen, g.states), "{}", g.id);
                let (lf, gf): (Vec<_>, Vec<_>) = (l.fields.iter().map(|f| (f.key, f.kind, f.required)).collect(), g.fields.iter().map(|f| (f.key, f.kind, f.required)).collect());
                assert_eq!(lf, gf, "{} fields", g.id);
                assert_eq!(l.icon == "bolt", g.icon == "bolt", "{} picture", g.id);
            }
            checked += 1;
        }
        assert!(checked > 0, "no plugins installed in {}", root.display());
    }
}
