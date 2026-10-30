// What a phone and the PC say to each other. The phone app is built against this.
//
// Transport: WebSocket over TLS on the port from Settings → Phones (47800 by default).
// The PC's certificate is self-signed; the phone trusts it only if its SHA-256 matches the
// fingerprint it got from the pairing QR code ("f=" in the link).
//
// Every message is one JSON text frame with a "type". The phone's first message is either
// "pair" (a new phone, with the code shown on the PC) or "hello" (a phone that is already paired).
//
//   phone → PC  {"type":"pair","v":1,"code":"123456","device":{"deviceId":"…","name":"Pixel 8","kind":"phone","os":"Android 15","appVersion":"1.0.0","aspect":2.22}}
//   PC → phone  {"type":"paired","phoneId":"…","token":"…","pcName":"DESKTOP-1"}   keep the token secret
//   phone → PC  {"type":"hello","v":1,"phoneId":"…","token":"…","rev":"<layout rev it has, or null>",
//                "has":["<rev>",…],"gz":true}
//                 has (optional): the layouts the phone keeps, so switching back to a profile sends
//                 nothing big. gz (optional): it takes a layout as a gzip binary frame (below).
//   PC → phone  {"type":"welcome","phoneId":"…","pcName":"DESKTOP-1"}
//   PC → phone  {"type":"error","code":"bad_code","message":"…"}                then the PC closes
//
// Once paired or welcomed the connection stays open, and the PC keeps the phone in sync:
//
//   PC → phone  {"type":"layout","rev":"…","page":"<page to show>","layout":{…},"media":[{"file","bytes","sha256"}]}
//                 the profile to show (see phones/sync.rs). Sent on connect unless "rev" is what
//                 the phone already has, and again whenever anything on it changes. Keep it, so
//                 the phone shows its buttons even before it reconnects. An icon or GIF id that
//                 starts with "plug:" comes from a plugin: layout.pluginMedia maps it to a media
//                 file (a picture or GIF) to show instead.
//                 To a phone that said gz, it is a binary frame instead: the same JSON, gzipped
//                 (2.8 MB of designs becomes about 100 KB; on a weak Wi-Fi the text took 15 s).
//   PC → phone  {"type":"known","rev":"…","page":"<page to show>"}
//                 show the layout with this rev that the phone kept (in "has", or sent before on
//                 this connection). A phone that doesn't have it any more answers needLayout.
//   phone → PC  {"type":"needLayout","rev":"…"}                                 send that layout in full
//   PC → phone  {"type":"page","page":"<page id>"}                              the PC turned the page
//   phone → PC  {"type":"getMedia","file":"…"}                                  a picture or GIF file
//   PC → phone  {"type":"media","file":"…","index":0,"total":3,"data":"<base64>"}  in parts of 512 KB
//   PC → phone  {"type":"mediaMissing","file":"…"}
//   phone → PC  {"type":"press","buttonId":"…","kind":"press"}                  a button was tapped
//                 kind (optional, "press" if missing): "press" for a tap, "long" when it is held
//                 (instead of "press"), "release" when the finger lifts after either
//   PC → phone  {"type":"ran","buttonId":"…","ok":true,"message":"Opened Notes"}  after "press" or "long"
//   PC → phone  {"type":"live","buttonId":"…","title":"Live 01:23"|null,"icon":"data:image/png;base64,…"|null}
//                 a plugin changed what a button shows; null means show its own text or icon again.
//                 Sent when it changes and after each layout; forget them when the layout changes.
//   PC → phone  {"type":"state","buttonId":"…","state":1}
//                 a plugin put a button in another of its states (a lamp lit). Show that state over
//                 the layout's; forget them when the layout changes (they're sent again after it).
//   PC → phone  {"type":"data","buttonId":"…","data":{…}|null}
//                 live data a button draws itself, such as {"kind":"map",…} from the MSFS plugin.
//                 Each key replaces the one the phone has; null clears it all. The whole of it is
//                 sent again after each layout.
//   phone → PC  {"type":"view","buttonId":"…","data":{"airport":"KATL"}}         what a live map on the
//                 button is looking at, passed to the button's plugin (a small object)
//   phone → PC  {"type":"viewing","page":"<page id>"}                            the page it shows now
//   phone → PC  {"type":"turn","buttonId":"…","by":2}                           a knob button turned by
//                 a drag: clicks, + clockwise (buttons with "knob": true)
//   phone → PC  {"type":"status","battery":76}
//   PC → phone  {"type":"updateAvailable","version":"1.1.0","notes":"…"}        a newer phone app exists
//   PC → phone  {"type":"portChanged","port":47801}                             the PC moved; reconnect there
//
// Error codes: unsupported_version, bad_request, no_pairing, expired, bad_code, too_many_tries,
// unknown_phone, bad_token, removed, replaced.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::sync::MediaFile;

pub const VERSION: u32 = 1;

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// Stays the same while the app is installed, so pairing again replaces the old entry
    pub device_id: String,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub app_version: String,
    /// Long side divided by short side of the screen
    #[serde(default)]
    pub aspect: f64,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Incoming {
    Pair { v: u32, code: String, device: DeviceInfo },
    Hello {
        v: u32,
        phone_id: String,
        token: String,
        #[serde(default)]
        rev: Option<String>,
        /// The layouts the phone keeps (their revs)
        #[serde(default)]
        has: Vec<String>,
        /// It takes a layout as a gzip binary frame
        #[serde(default)]
        gz: bool,
    },
    Status { battery: Option<u8> },
    Press {
        button_id: String,
        #[serde(default)]
        kind: Option<String>,
    },
    GetMedia { file: String },
    /// What a live map on a button is looking at, such as {"airport":"KATL"}, for its plugin
    View { button_id: String, data: Value },
    /// A knob button turned by a drag: clicks, more than 0 clockwise
    Turn { button_id: String, by: i32 },
    /// The page the phone shows now, so its buttons' plugins send it their live values
    Viewing { page: String },
    /// A "known" layout the phone no longer keeps: send it in full
    NeedLayout { rev: String },
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Outgoing {
    Paired { phone_id: String, token: String, pc_name: String },
    Welcome { phone_id: String, pc_name: String },
    Error { code: String, message: String },
    Layout { rev: String, page: String, layout: Value, media: Vec<MediaFile> },
    /// Show the layout with this rev that the phone kept
    Known { rev: String, page: String },
    Page { page: String },
    Media { file: String, index: usize, total: usize, data: String },
    MediaMissing { file: String },
    Ran { button_id: String, ok: bool, message: String },
    UpdateAvailable { version: String, notes: String },
    PortChanged { port: u16 },
    Live { button_id: String, title: Option<String>, icon: Option<String> },
    Data { button_id: String, data: Value },
    /// The state a plugin put a button in (a lamp lit, a switch's position), on its own: a
    /// whole layout for each change was far too slow
    State { button_id: String, state: u64 },
}

impl Outgoing {
    pub fn error(code: &str, message: &str) -> Self {
        Outgoing::Error { code: code.into(), message: message.into() }
    }
    pub fn text(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_read_and_write_as_documented() {
        let m: Incoming = serde_json::from_str(r#"{"type":"pair","v":1,"code":"123 456","device":{"deviceId":"d1","name":"Pixel 8","kind":"phone","os":"Android 15","appVersion":"1.0.0","aspect":2.22}}"#).unwrap();
        assert!(matches!(m, Incoming::Pair { v: 1, ref code, ref device } if code == "123 456" && device.device_id == "d1"));
        let m: Incoming = serde_json::from_str(r#"{"type":"hello","v":1,"phoneId":"p1","token":"t"}"#).unwrap();
        assert!(matches!(m, Incoming::Hello { ref phone_id, .. } if phone_id == "p1"));
        let out = Outgoing::Welcome { phone_id: "p1".into(), pc_name: "PC".into() }.text();
        assert_eq!(out, r#"{"type":"welcome","phoneId":"p1","pcName":"PC"}"#);
        let m: Incoming = serde_json::from_str(r#"{"type":"press","buttonId":"t1"}"#).unwrap();
        assert!(matches!(m, Incoming::Press { ref button_id, kind: None } if button_id == "t1"));
        let m: Incoming = serde_json::from_str(r#"{"type":"press","buttonId":"t1","kind":"long"}"#).unwrap();
        assert!(matches!(m, Incoming::Press { kind: Some(ref k), .. } if k == "long"));
        let m: Incoming = serde_json::from_str(r#"{"type":"view","buttonId":"t1","data":{"airport":"KATL"}}"#).unwrap();
        assert!(matches!(m, Incoming::View { ref button_id, ref data } if button_id == "t1" && data["airport"] == "KATL"));
        let v: Incoming = serde_json::from_str(r#"{"type":"viewing","page":"p-fnx-2"}"#).unwrap();
        assert!(matches!(v, Incoming::Viewing { ref page } if page == "p-fnx-2"));
        let m: Incoming = serde_json::from_str(r#"{"type":"turn","buttonId":"t1","by":-3}"#).unwrap();
        assert!(matches!(m, Incoming::Turn { ref button_id, by: -3 } if button_id == "t1"));
        let out = Outgoing::Live { button_id: "t1".into(), title: Some("Live".into()), icon: None }.text();
        assert_eq!(out, r#"{"type":"live","buttonId":"t1","title":"Live","icon":null}"#);
        let out = Outgoing::Data { button_id: "t1".into(), data: serde_json::json!({ "kind": "map" }) }.text();
        assert_eq!(out, r#"{"type":"data","buttonId":"t1","data":{"kind":"map"}}"#);
        let out = Outgoing::State { button_id: "t1".into(), state: 2 }.text();
        assert_eq!(out, r#"{"type":"state","buttonId":"t1","state":2}"#);
        let m: Incoming = serde_json::from_str(r#"{"type":"hello","v":1,"phoneId":"p1","token":"t","has":["r1","r2"],"gz":true}"#).unwrap();
        assert!(matches!(m, Incoming::Hello { ref has, gz: true, .. } if has.len() == 2));
        let m: Incoming = serde_json::from_str(r#"{"type":"needLayout","rev":"r1"}"#).unwrap();
        assert!(matches!(m, Incoming::NeedLayout { ref rev } if rev == "r1"));
        let out = Outgoing::Known { rev: "r1".into(), page: "p1".into() }.text();
        assert_eq!(out, r#"{"type":"known","rev":"r1","page":"p1"}"#);
        let out = Outgoing::Ran { button_id: "t1".into(), ok: true, message: "Done".into() }.text();
        assert_eq!(out, r#"{"type":"ran","buttonId":"t1","ok":true,"message":"Done"}"#);
    }
}
