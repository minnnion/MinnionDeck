// ===== Live map buttons =====
// A plugin can send a button live data to draw (Host::set_data). {"kind":"map", ...} from the
// MSFS 2024 plugin draws a map: the aircraft, the route planned in the EFB, and the trail flown.
//   bg       "outline" (the world outline, built in) or "osm" (street map tiles)
//   tiles    the street map tile address, with {z}, {x} and {y}
//   zoom     where the map starts: "Close", "Medium", "Far" or "Whole route"
//   status   a word when the sim isn't connected ("MSFS off"), else ""
//   plane    {lat, lon, hdg, gs, alt} or null
//   route    {rev, points: [{n, k: dep|fix|point|dest, lat, lon}]}
//   trail    [[lat, lon], ...]
//   info     how the EFB route stands ("No route in the EFB yet..."), shown while there's no route
//   airports [[ident, lat, lon], ...] the sim's airports around the aircraft
//   apt:XXXX the sim's diagram of an airport (runways, taxiways, gates): see the plugin's airport.rs
//   taxi     on the ground: the taxi route as ATC cleared it (src "atc", to "Gate 113" or "RWY 16R",
//            clr the clearance's words), else to the EFB's departure runway
//            {apt, rwy, to, pts: [[lat, lon], ...], via: [taxiway names], hold: [[lat, lon, heading]], m},
//            or {note} saying why there's none (see the plugin's taxi.rs and traffic.rs)
//   traffic  other aircraft: [[key, callsign, lat, lon, heading, altitude, ground speed, on ground,
//            category L|P|M|H|J], ...]; they glide on between updates along their heading
//   sel      the aircraft tapped: its flight, state and taxi route (see the plugin's traffic.rs)
//   ofp      the SimBrief route {pts: [[ident, lat, lon], ...]}, dashed
//   push     the pushback ATC gave ("Face west"): {pts: [[lat, lon], ...], ghost: [lat, lon, hdg],
//            face "W", tail "L"|"R", label, gate} or {face, tail, label, note} with no way to draw;
//            the aircraft lights up, and a tap on it shows the way (see the plugin's push.rs)
// The world outline also has lakes, rivers, state lines and cities (00-world.js); every airport
// and its runways come from public/airports.json (OurAirports).
// Each update replaces the keys it has. A map is a screen, not a button: drag it to move, the wheel
// or + and - to zoom, the target to follow the aircraft again; tap an aircraft for its card.
// Other screens a plugin can send: {"kind":"ofp"} a flight plan to scroll, {"kind":"atc"} radio
// messages, {"kind":"gsx"} GSX's menu to pick from, and the Spotify plugin's {"kind":"spotify"}
// now playing and {"kind":"spotify_list"} lists to play from (see the end of this file).
const LIVE_DATA = {};
const MAP_VIEW = {};
const MAP_ZOOM = { Airport: 14, Close: 11, Medium: 9, Far: 7 };
const SCREEN_KINDS = new Set(["map", "ofp", "atc", "gsx", "spotify", "spotify_list"]);

function mergeLiveData(id, data) {
  if (data == null) { delete LIVE_DATA[id]; delete MAP_VIEW[id]; return; }
  LIVE_DATA[id] = { ...(LIVE_DATA[id] || {}), ...data };
  // When the traffic came, so aircraft glide on from there
  if (data.traffic) LIVE_DATA[id]._trafficAt = Date.now();
}
const mapDataOf = (t) => { const d = LIVE_DATA[t.id]; return d && d.kind === "map" ? d : null; };
const screenDataOf = (t) => { const d = LIVE_DATA[t.id]; return d && SCREEN_KINDS.has(d.kind) ? d : null; };
const mapView = (id) => MAP_VIEW[id] || (MAP_VIEW[id] = { zoom: null, center: null, explore: false, traffic: true, labels: true, hits: [] });

// The world outline, unpacked once into degrees with each shape's bounds
let WORLD_SHAPES = null;
function worldShapes() {
  if (WORLD_SHAPES) return WORLD_SHAPES;
  const unpack = (list) => list.map((a) => {
    const pts = new Float32Array(a.length);
    let x = 0, y = 0, minX = 1e9, minY = 1e9, maxX = -1e9, maxY = -1e9;
    for (let i = 0; i < a.length; i += 2) {
      x += a[i]; y += a[i + 1];
      const lon = x / WORLD.q, lat = y / WORLD.q;
      pts[i] = lon; pts[i + 1] = lat;
      minX = Math.min(minX, lon); maxX = Math.max(maxX, lon); minY = Math.min(minY, lat); maxY = Math.max(maxY, lat);
    }
    return { pts, minX, minY, maxX, maxY };
  });
  const places = (list) => (list || []).map(([n, x, y, z]) => ({ n, lat: y / WORLD.q, lon: x / WORLD.q, z }));
  return (WORLD_SHAPES = {
    land: unpack(WORLD.land), borders: unpack(WORLD.borders), states: unpack(WORLD.states || []),
    lakes: unpack(WORLD.lakes || []), rivers: unpack(WORLD.rivers || []),
    cities: places(WORLD.cities), airports: places(WORLD.airports),
  });
}

// The lat/lon grid's spacing in degrees at a zoom level
const gridStep = (z) => (z < 3 ? 30 : z < 5 ? 10 : z < 7 ? 5 : z < 9 ? 1 : z < 11 ? 0.5 : 0.25);

// Labels that would cover one already drawn are left out
function labelSpace() {
  const taken = [];
  return (x, y, w, h) => {
    if (taken.some((b) => x < b[0] + b[2] && x + w > b[0] && y < b[1] + b[3] && y + h > b[1])) return false;
    taken.push([x, y, w, h]);
    return true;
  };
}

// Web Mercator in pixels at a zoom level (256 px tiles), so street map tiles line up
const merc = (lat, lon, z) => {
  const s = 256 * 2 ** z, l = Math.max(-85, Math.min(85, lat)) * Math.PI / 180;
  return [((lon + 180) / 360) * s, (0.5 - Math.log(Math.tan(Math.PI / 4 + l / 2)) / (2 * Math.PI)) * s];
};
const unmerc = (x, y, z) => {
  const s = 256 * 2 ** z;
  return [Math.atan(Math.sinh(Math.PI * (1 - (2 * y) / s))) * 180 / Math.PI, (x / s) * 360 - 180];
};

// The zoom that shows the whole route (and the aircraft) in a w × h box
function fitZoom(d, w, h) {
  const pts = [...((d.route && d.route.points) || []).map((p) => [p.lat, p.lon]), ...(d.plane ? [[d.plane.lat, d.plane.lon]] : [])];
  if (pts.length < 2) return { zoom: MAP_ZOOM.Medium, center: pts[0] || null };
  for (let z = 14; z >= 1; z--) {
    const xy = pts.map(([la, lo]) => merc(la, lo, z));
    const xs = xy.map((p) => p[0]), ys = xy.map((p) => p[1]);
    if (Math.max(...xs) - Math.min(...xs) < w * 0.85 && Math.max(...ys) - Math.min(...ys) < h * 0.85) {
      return { zoom: z, center: unmerc((Math.max(...xs) + Math.min(...xs)) / 2, (Math.max(...ys) + Math.min(...ys)) / 2, z) };
    }
  }
  return { zoom: 1, center: [0, 0] };
}

// Street map tiles, kept while the app runs; asked for through the app (which caches them on disk)
const TILE_IMG = new Map();
function tileImg(tpl, z, x, y, redraw) {
  const key = `${tpl}|${z}/${x}/${y}`;
  const have = TILE_IMG.get(key);
  if (have) return have.img && have.img.complete ? have.img : null;
  const entry = { img: null };
  TILE_IMG.set(key, entry);
  backend.call("map_tile", { template: tpl, z, x, y }, null).then((url) => {
    if (!url) return;
    const img = new Image();
    img.onload = redraw;
    img.src = url;
    entry.img = img;
  });
  if (TILE_IMG.size > 600) TILE_IMG.delete(TILE_IMG.keys().next().value);
  return null;
}

const cssVar = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim() || "#888";

// ----- Every airport (public/airports.json, from OurAirports, public domain: ourairports.com) -----
//   q  units per degree; a: [ident, type, lat, lon, name, elevation ft, iata] with type L large,
//   M medium, S small, H heliport, W seaplane base, B balloon port; r: runways [airport, lat1, lon1,
//   lat2, lon2, width ft, end 1, end 2, surface 0 hard / 1 soft / 2 water].
// Loaded the first time a map is drawn, and kept in 1° squares so a map finds its own quickly.
let OA = null, OA_LOADING = false;
const oaCell = (lat, lon) => `${Math.floor(lat)},${Math.floor(lon)}`;
function oaData() {
  if (OA || OA_LOADING) return OA;
  OA_LOADING = true;
  fetch("airports.json").then((r) => r.json()).then((j) => {
    const q = j.q, cells = new Map(), mid = [], big = [];
    const all = j.a.map(([id, t, la, lo, name, elev, iata]) => {
      const a = { id, t, lat: la / q, lon: lo / q, name, elev, iata, rw: null };
      const k = oaCell(a.lat, a.lon);
      if (!cells.has(k)) cells.set(k, []);
      cells.get(k).push(a);
      if (t === "L") big.push(a);
      if (t === "L" || t === "M") mid.push(a);
      return a;
    });
    j.r.forEach(([ai, a1, o1, a2, o2, wft, e1, e2, s]) => { const a = all[ai]; if (a) (a.rw || (a.rw = [])).push([a1 / q, o1 / q, a2 / q, o2 / q, wft * 0.3048, e1, e2, s]); });
    OA = { cells, mid, big, ids: new Set(all.map((a) => a.id)) };
    Object.keys(LIVE_DATA).forEach(drawMaps);
  }).catch(() => { OA_LOADING = false; });
  return null;
}
// Airports shown from these zooms: a dot, then its code (and name for big ones further in)
const OA_SHOW = { L: [3, 5], M: [6, 8], S: [8, 10], W: [8, 10], H: [11, 13], B: [11, 13] };
function oaInView(la1, lo1, la2, lo2, zoom) {
  if (zoom < 6) return OA.big;
  if (zoom < 8 || (la2 - la1) * (lo2 - lo1) > 300) return OA.mid;
  const out = [];
  for (let la = Math.floor(la1); la <= Math.floor(la2); la++) for (let lo = Math.floor(lo1); lo <= Math.floor(lo2); lo++) {
    const c = OA.cells.get(`${la},${((((lo + 180) % 360) + 360) % 360) - 180}`);
    if (c) out.push(...c);
  }
  return out;
}

// Metres per screen pixel at a latitude and zoom
const mPerPx = (lat, zoom) => (156543.03 * Math.cos((lat * Math.PI) / 180)) / 2 ** zoom;

// A runway as a band its true width (at least 2 px), with its end numbers when close in
function drawRunway(g, at, zoom, mpp, r, colour, ink, room, numbers) {
  const [x1, y1] = at(r[0], r[1]), [x2, y2] = at(r[2], r[3]);
  g.lineCap = "butt";
  g.strokeStyle = colour;
  g.lineWidth = Math.max(2, (r[4] || 30) / mpp);
  g.beginPath(); g.moveTo(x1, y1); g.lineTo(x2, y2); g.stroke();
  // A dashed white centre line on a hard runway, close in
  if (zoom >= 14 && r[7] !== 1 && r[7] !== 2) {
    g.strokeStyle = "#ffffff"; g.lineWidth = Math.max(1, 1 / mpp);
    g.setLineDash([Math.max(3, 36 / mpp), Math.max(3, 24 / mpp)]);
    g.beginPath(); g.moveTo(x1, y1); g.lineTo(x2, y2); g.stroke();
    g.setLineDash([]);
  }
  if (!numbers) return;
  const len = Math.hypot(x2 - x1, y2 - y1) || 1, ux = (x2 - x1) / len, uy = (y2 - y1) / len;
  g.font = `700 10px "IBM Plex Mono", monospace`;
  g.textBaseline = "middle"; g.textAlign = "center";
  [[x1, y1, -1, r[5]], [x2, y2, 1, r[6]]].forEach(([x, y, s, n]) => {
    if (!n) return;
    const tx = x + ux * s * 12, ty = y + uy * s * 12, tw = g.measureText(n).width + 6;
    if (!room(tx - tw / 2, ty - 7, tw, 14)) return;
    g.fillStyle = ink; g.fillRect(tx - tw / 2, ty - 7, tw, 14);
    g.fillStyle = "#ffffff"; g.fillText(n, tx, ty);
  });
  g.textAlign = "start";
}

// The sim's diagram of an airport: taxiways (with their names), runways, gates and stands.
// Points come in metres east and north of the airport's reference point; they're turned into
// degrees once per diagram.
const DIAGRAM_PTS = new WeakMap();
function drawDiagram(g, a, at, zoom, mpp, w, h, room, ink) {
  let pts = DIAGRAM_PTS.get(a);
  if (!pts) {
    const k = 111320 * Math.max(0.01, Math.cos((a.lat * Math.PI) / 180));
    pts = new Float64Array(a.pt.length);
    for (let i = 0; i < a.pt.length; i += 2) { pts[i] = a.lat + a.pt[i + 1] / 111320; pts[i + 1] = a.lon + a.pt[i] / k; }
    DIAGRAM_PTS.set(a, pts);
  }
  const xy = (i) => at(pts[i * 2], pts[i * 2 + 1]);
  const tw = a.tw || [];
  // Taxiways: grey bands their true width, a yellow centre line close in
  g.lineCap = "round";
  g.strokeStyle = cssVar("--slate"); g.globalAlpha = 0.55;
  for (let i = 0; i < tw.length; i += 4) {
    const [x1, y1] = xy(tw[i]), [x2, y2] = xy(tw[i + 1]);
    if ((x1 < -50 && x2 < -50) || (y1 < -50 && y2 < -50) || (x1 > w + 50 && x2 > w + 50) || (y1 > h + 50 && y2 > h + 50)) continue;
    g.lineWidth = Math.max(1.5, (tw[i + 2] || 20) / mpp);
    g.beginPath(); g.moveTo(x1, y1); g.lineTo(x2, y2); g.stroke();
  }
  g.globalAlpha = 1;
  if (zoom >= 14) {
    g.strokeStyle = cssVar("--sun"); g.lineWidth = 1;
    g.beginPath();
    for (let i = 0; i < tw.length; i += 4) { const [x1, y1] = xy(tw[i]), [x2, y2] = xy(tw[i + 1]); g.moveTo(x1, y1); g.lineTo(x2, y2); }
    g.stroke();
  }
  (a.rw || []).forEach((r) => drawRunway(g, at, zoom, mpp, r, ink, ink, room, zoom >= 12));
  // Gates and stands: dots, with their numbers when there's room
  if (zoom >= 14) {
    const colour = { g: cssVar("--lilac"), c: cssVar("--peach"), f: cssVar("--sun"), r: cssVar("--mint") };
    g.font = `600 9px "IBM Plex Mono", monospace`; g.textBaseline = "middle";
    (a.pk || []).forEach(([p, , radius, label, kind]) => {
      const [x, y] = xy(p);
      if (x < -10 || y < -10 || x > w + 10 || y > h + 10) return;
      const r = Math.max(2.5, Math.min(10, (radius || 10) / mpp / 2));
      g.beginPath(); g.arc(x, y, r, 0, Math.PI * 2);
      g.fillStyle = colour[kind] || colour.r; g.fill();
      g.strokeStyle = ink; g.lineWidth = 1; g.stroke();
      if (zoom >= 15 && label && room(x + r + 1, y - 6, g.measureText(label).width + 4, 12)) { g.fillStyle = ink; g.fillText(label, x + r + 3, y); }
    });
  }
  // Taxiway names: one yellow box per name, on its middle piece
  if (zoom >= 14 && a.tn) {
    const byName = new Map();
    for (let i = 0; i < tw.length; i += 4) if (tw[i + 3] >= 0) { if (!byName.has(tw[i + 3])) byName.set(tw[i + 3], []); byName.get(tw[i + 3]).push(i); }
    g.font = `700 10px "IBM Plex Mono", monospace`; g.textBaseline = "middle"; g.textAlign = "center";
    byName.forEach((list, n) => {
      const name = a.tn[n];
      if (!name) return;
      const i = list[Math.floor(list.length / 2)];
      const [x1, y1] = xy(tw[i]), [x2, y2] = xy(tw[i + 1]);
      const x = (x1 + x2) / 2, y = (y1 + y2) / 2, bw = g.measureText(name).width + 6;
      if (x < 0 || y < 0 || x > w || y > h || !room(x - bw / 2, y - 7, bw, 14)) return;
      g.fillStyle = ink; g.fillRect(x - bw / 2, y - 7, bw, 14);
      g.fillStyle = cssVar("--sun"); g.fillText(name, x, y);
    });
    g.textAlign = "start";
  }
}

// Zoomed in on an airport with no diagram yet: ask the plugin for it (once in a while, so a
// slow or missing answer isn't asked for over and over)
const DIAGRAM_ASKED = new Map();
function askDiagram(id, airport) {
  const key = `${id}|${airport}`, last = DIAGRAM_ASKED.get(key) || 0;
  if (Date.now() - last < 15000 || !window.mapLooksAt) return;
  DIAGRAM_ASKED.set(key, Date.now());
  window.mapLooksAt(id, { airport });
}

// Airports: the sim's diagrams close in, every airport's runways from zoom 9, and a dot and code
// for the rest (bigger airports from further out). The sim's nearby airports fill any gaps.
function drawAirports(g, d, zoom, w, h, at, room, ink, view, id) {
  const [la1, lo1, la2, lo2] = view;
  const mpp = mPerPx((la1 + la2) / 2, zoom);
  const inside = (x, y) => x > -4 && y > -4 && x < w + 4 && y < h + 4;
  const diagrams = Object.keys(d).filter((k) => k.startsWith("apt:") && d[k] && d[k].pt).map((k) => d[k]);
  const withDiagram = new Set(diagrams.map((a) => a.id));
  if (zoom >= 11) diagrams.forEach((a) => drawDiagram(g, a, at, zoom, mpp, w, h, room, ink));
  const oa = oaData();
  const list = oa ? oaInView(la1 - 0.1, lo1 - 0.1, la2 + 0.1, lo2 + 0.1, zoom) : [];
  // Close in, the airport nearest the middle of the map gets its diagram from the sim
  if (zoom >= 12 && id) {
    const cla = (la1 + la2) / 2, clo = (lo1 + lo2) / 2, k = Math.cos((cla * Math.PI) / 180);
    let best = null, bd = Infinity;
    for (const a of list) {
      if (a.t === "H" || a.t === "B" || a.lat < la1 || a.lat > la2 || a.lon < lo1 || a.lon > lo2) continue;
      const d2 = (a.lat - cla) ** 2 + ((a.lon - clo) * k) ** 2;
      if (d2 < bd) { bd = d2; best = a; }
    }
    if (best && !withDiagram.has(best.id)) askDiagram(id, best.id);
  }
  const hard = cssVar("--slate"), soft = cssVar("--mint-dark"), water = cssVar("--sky-dark");
  // Runways of every airport in view, from zoom 9
  if (zoom >= 9) list.forEach((a) => {
    if (!a.rw || (zoom >= 11 && withDiagram.has(a.id))) return;
    const [x, y] = at(a.lat, a.lon);
    if (x < -200 || y < -200 || x > w + 200 || y > h + 200) return;
    a.rw.forEach((r) => drawRunway(g, at, zoom, mpp, r, r[7] === 1 ? soft : r[7] === 2 ? water : hard, ink, room, zoom >= 13));
  });
  // Codes (and names close in) for big airports first, then the rest
  const order = { L: 0, M: 1, S: 2, W: 2, H: 3, B: 3 };
  const shown = list.filter((a) => zoom >= OA_SHOW[a.t][0]).sort((p, q) => order[p.t] - order[q.t]);
  const extra = oa ? (d.airports || []).filter(([n]) => !oa.ids.has(n)).map(([id, lat, lon]) => ({ id, t: "S", lat, lon, name: "" })) : (d.airports || []).map(([id, lat, lon]) => ({ id, t: "S", lat, lon, name: "" }));
  g.textBaseline = "middle";
  [...shown, ...(zoom >= 8 ? extra : [])].forEach((a) => {
    const [x, y] = at(a.lat, a.lon);
    if (!inside(x, y)) return;
    const drawnAsRunways = (zoom >= 9 && a.rw) || (zoom >= 11 && withDiagram.has(a.id));
    if (!drawnAsRunways) {
      const r = a.t === "L" ? 4 : a.t === "M" ? 3.5 : 3;
      g.beginPath(); g.arc(x, y, r, 0, Math.PI * 2);
      g.fillStyle = a.t === "H" || a.t === "B" ? cssVar("--peach") : a.t === "W" ? cssVar("--sky") : cssVar("--lilac"); g.fill();
      g.strokeStyle = ink; g.lineWidth = 1.5; g.stroke();
    }
    if (zoom < OA_SHOW[a.t][1]) return;
    const label = zoom >= 11 && a.name && (a.t === "L" || a.t === "M" || zoom >= 13) ? `${a.id} ${a.name}` : a.id;
    g.font = `${a.t === "L" ? 700 : 600} 9px "IBM Plex Mono", monospace`;
    const tw = g.measureText(label).width;
    const lx = drawnAsRunways ? x - tw / 2 : x + 6, ly = drawnAsRunways ? y - 14 : y;
    if (!room(lx - 2, ly - 6, tw + 4, 12)) return;
    if (drawnAsRunways) { g.fillStyle = cssVar("--panel"); g.globalAlpha = 0.85; g.fillRect(lx - 2, ly - 6, tw + 4, 12); g.globalAlpha = 1; }
    g.fillStyle = ink; g.fillText(label, lx, ly);
  });
}

// Cities on the world outline; labels go only where there's room
function drawPlaces(g, d, zoom, w, h, at, room, ink) {
  const W = worldShapes();
  const inside = (x, y) => x > -4 && y > -4 && x < w + 4 && y < h + 4;
  g.textBaseline = "middle";
  g.font = `500 10px "IBM Plex Mono", monospace`;
  W.cities.forEach((c) => {
    if (zoom < c.z) return;
    const [x, y] = at(c.lat, c.lon);
    if (!inside(x, y)) return;
    g.fillStyle = cssVar("--slate");
    g.fillRect(x - 2, y - 2, 4, 4);
    if (room(x + 4, y - 6, g.measureText(c.n).width + 4, 12)) g.fillText(c.n, x + 6, y);
  });
}

// A short note in a box across the top of the map, wrapped to fit
function drawInfo(g, text, w, ink) {
  g.font = `600 10px "IBM Plex Mono", monospace`;
  g.textBaseline = "top";
  const lines = [];
  let line = "";
  text.split(/\s+/).forEach((word) => {
    const next = line ? `${line} ${word}` : word;
    if (line && g.measureText(next).width > w - 28) { lines.push(line); line = word; } else line = next;
  });
  if (line) lines.push(line);
  const shown = lines.slice(0, 3);
  g.fillStyle = cssVar("--panel"); g.fillRect(8, 8, w - 16, shown.length * 13 + 8);
  g.strokeStyle = ink; g.lineWidth = 1.5; g.strokeRect(8, 8, w - 16, shown.length * 13 + 8);
  g.fillStyle = ink;
  shown.forEach((l, i) => g.fillText(l, 14, 12 + i * 13));
}

// ----- The taxi route -----
const taxiPts = (d) => (d.taxi && Array.isArray(d.taxi.pts) && d.taxi.pts.length > 1 ? d.taxi.pts : null);
const taxiLength = (m) => (m >= 1000 ? `${(m / 1000).toFixed(1)} km` : `${Math.round(m)} m`);

// The zoom and middle that show the whole taxi route, leaving room for its card at the top
function fitTaxi(d, w, h) {
  const pts = taxiPts(d);
  for (let z = 17; z >= 11; z--) {
    const xy = pts.map(([la, lo]) => merc(la, lo, z));
    const xs = xy.map((p) => p[0]), ys = xy.map((p) => p[1]);
    if (Math.max(...xs) - Math.min(...xs) < w * 0.8 && Math.max(...ys) - Math.min(...ys) < (h - 50) * 0.8) {
      return { zoom: z, center: unmerc((Math.max(...xs) + Math.min(...xs)) / 2, (Math.max(...ys) + Math.min(...ys)) / 2 - 22, z) };
    }
  }
  const [la, lo] = pts[0];
  return { zoom: 11, center: [la, lo] };
}

// The route: a yellow line with ink edges and arrows along it, hold-short bars across it, and the
// runway it goes to
function drawTaxi(g, t, at, zoom, room, ink, colour = cssVar("--sun")) {
  const pts = t.pts.map(([la, lo]) => at(la, lo));
  g.lineJoin = "round"; g.lineCap = "round";
  g.beginPath();
  pts.forEach(([x, y], i) => (i ? g.lineTo(x, y) : g.moveTo(x, y)));
  g.strokeStyle = ink; g.lineWidth = 8; g.stroke();
  g.strokeStyle = colour; g.lineWidth = 5; g.stroke();
  // Arrows every 70 px, pointing the way to go
  g.fillStyle = ink;
  let next = 40;
  for (let i = 1; i < pts.length; i++) {
    const [x1, y1] = pts[i - 1], [x2, y2] = pts[i], len = Math.hypot(x2 - x1, y2 - y1);
    const a = Math.atan2(y2 - y1, x2 - x1);
    while (next < len) {
      const x = x1 + ((x2 - x1) * next) / len, y = y1 + ((y2 - y1) * next) / len;
      g.save(); g.translate(x, y); g.rotate(a);
      g.beginPath(); g.moveTo(3, 0); g.lineTo(-2, -3); g.lineTo(-2, 3); g.closePath(); g.fill();
      g.restore();
      next += 70;
    }
    next -= len;
  }
  // Hold-short lines: coral bars across the way
  g.font = `700 8px "IBM Plex Mono", monospace`; g.textBaseline = "middle"; g.textAlign = "center";
  (t.hold || []).forEach(([la, lo, hdg]) => {
    const [x, y] = at(la, lo);
    g.save(); g.translate(x, y); g.rotate((hdg * Math.PI) / 180);
    g.fillStyle = ink; g.fillRect(-11, -5, 22, 10);
    g.fillStyle = cssVar("--coral"); g.fillRect(-10, -4, 20, 3); g.fillRect(-10, 1, 20, 3);
    g.restore();
    if (zoom >= 14 && room(x + 12, y - 6, 28, 12)) {
      g.fillStyle = cssVar("--coral"); g.fillRect(x + 12, y - 6, 28, 12);
      g.fillStyle = "#ffffff"; g.fillText("HOLD", x + 26, y);
    }
  });
  // The runway it goes to: a flag and its number, just past the end of the route (so it covers
  // neither the route nor the hold-short line before the runway)
  const [ex, ey] = pts[pts.length - 1], [px, py] = pts[pts.length - 2];
  const el = Math.hypot(ex - px, ey - py) || 1, ux = (ex - px) / el, uy = (ey - py) / el;
  g.font = `700 11px "Pixelify Sans", "IBM Plex Mono", monospace`;
  const label = t.to || `RWY ${t.rwy}`, tw = g.measureText(label).width + 22;
  const bx = ex + ux * (tw / 2 + 10) - tw / 2, by = ey + uy * 22 - 9;
  room(bx, by, tw, 18);
  g.fillStyle = ink; g.fillRect(bx + 2, by + 2, tw, 18);
  g.fillRect(bx, by, tw, 18);
  g.fillStyle = cssVar("--sun");
  g.fillRect(bx + 5, by + 4, 2, 11);
  g.beginPath(); g.moveTo(bx + 7, by + 4); g.lineTo(bx + 14, by + 7); g.lineTo(bx + 7, by + 10); g.closePath(); g.fill();
  g.fillStyle = "#ffffff"; g.fillText(label, bx + 12 + (tw - 12) / 2, by + 9);
  g.textAlign = "start";
  g.beginPath(); g.arc(ex, ey, 4, 0, Math.PI * 2); g.fillStyle = cssVar("--sun"); g.fill(); g.strokeStyle = ink; g.lineWidth = 2; g.stroke();
}

// The taxi card: a TAXI tag, where to and which way, or why there's no route. At the top, or at
// the bottom while exploring (the buttons are at the top then).
function drawTaxiCard(g, t, w, h, exploring, ink) {
  const lines = t.note ? [t.note] : [`${t.src === "atc" ? "ATC: " : ""}to ${t.to || `RWY ${t.rwy}`}`, `${t.via && t.via.length ? `via ${t.via.join(" ")} · ` : ""}${taxiLength(t.m || 0)}`];
  g.font = `600 10px "IBM Plex Mono", monospace`;
  const wrapped = [];
  const maxW = w - 16 - 52;
  lines.forEach((text) => {
    let line = "";
    text.split(/\s+/).forEach((word) => {
      const next = line ? `${line} ${word}` : word;
      if (line && g.measureText(next).width > maxW) { wrapped.push(line); line = word; } else line = next;
    });
    if (line) wrapped.push(line);
  });
  const shown = wrapped.slice(0, 3), ch = shown.length * 13 + 10, y = exploring ? h - 36 - ch : 8;
  g.fillStyle = ink; g.fillRect(10, y + 2, w - 16, ch);
  g.fillStyle = cssVar("--panel"); g.fillRect(8, y, w - 16, ch);
  g.strokeStyle = ink; g.lineWidth = 2; g.strokeRect(8, y, w - 16, ch);
  g.fillStyle = t.src === "atc" ? cssVar("--mint") : cssVar("--sun"); g.fillRect(13, y + 5, 40, 14);
  g.fillStyle = ink; g.font = `700 10px "Pixelify Sans", "IBM Plex Mono", monospace`; g.textBaseline = "middle";
  g.fillText(t.src === "atc" ? "ATC" : "TAXI", t.src === "atc" ? 22 : 18, y + 12);
  g.font = `600 10px "IBM Plex Mono", monospace`; g.textBaseline = "top";
  shown.forEach((l, i) => g.fillText(l, 58, y + 6 + i * 13));
}

// ----- Other aircraft -----
// Our own aircraft shapes, seen from above with the nose up, one size per category, in px
const PLANE_SIZE = { L: 9, P: 11, M: 13, H: 16, J: 18 };
function planeShape(g, s, cat) {
  const k = s / 16;
  const P = (pts) => { g.beginPath(); pts.forEach(([x, y], i) => (i ? g.lineTo(x * k, y * k) : g.moveTo(x * k, y * k))); g.closePath(); };
  if (cat === "L" || cat === "P") {
    // Straight wings across the middle, a small tail
    P([[0, -9], [1.5, -7], [1.5, -3], [9, -3], [9, 0], [1.5, 0], [1, 6], [4, 7], [4, 9], [0, 8.5], [-4, 9], [-4, 7], [-1, 6], [-1.5, 0], [-9, 0], [-9, -3], [-1.5, -3], [-1.5, -7]]);
  } else {
    // Swept wings and tail
    P([[0, -10], [1.6, -7], [1.6, -3], [10, 2], [10, 4], [1.6, 1], [1.2, 6], [4.5, 8.5], [4.5, 10], [0, 8.8], [-4.5, 10], [-4.5, 8.5], [-1.2, 6], [-1.6, 1], [-10, 4], [-10, 2], [-1.6, -3], [-1.6, -7]]);
  }
}

// Where an aircraft is now: from where it was sent, on along its heading at its ground speed
function trafficNow(d, row) {
  const [, , lat, lon, hdg, , gs, ground] = row;
  const dt = Math.min(4, Math.max(0, (Date.now() - (d._trafficAt || Date.now())) / 1000));
  if (!gs || (ground && gs < 2) || !dt) return [lat, lon];
  const m = gs * 0.514444 * dt, h = (hdg * Math.PI) / 180;
  return [lat + (m * Math.cos(h)) / 111320, lon + (m * Math.sin(h)) / (111320 * Math.max(0.01, Math.cos((lat * Math.PI) / 180)))];
}

function drawTraffic(g, d, v, at, zoom, w, h, room, ink) {
  v.hits = [];
  if (!v.traffic || !Array.isArray(d.traffic)) return;
  const sel = d.sel && d.sel.k;
  g.textBaseline = "middle";
  d.traffic.forEach((row) => {
    const [key, cs, , , hdg, alt, , ground, cat] = row;
    // On the ground, aircraft show close in only (a ramp full of them far out is noise)
    if (ground && zoom < 12 && key !== sel) return;
    const [la, lo] = trafficNow(d, row);
    const [x, y] = at(la, lo);
    if (x < -20 || y < -20 || x > w + 20 || y > h + 20) return;
    const s = PLANE_SIZE[cat] || 13;
    g.save(); g.translate(x, y); g.rotate(((hdg || 0) * Math.PI) / 180);
    planeShape(g, s * (zoom >= 15 ? 1.4 : 1), cat);
    g.fillStyle = key === sel ? cssVar("--coral") : ground ? cssVar("--sun") : cssVar("--sky");
    g.strokeStyle = ink; g.lineWidth = 1.5;
    g.fill(); g.stroke();
    g.restore();
    if (key === sel) { g.strokeStyle = cssVar("--coral"); g.lineWidth = 2; g.strokeRect(x - s - 3, y - s - 3, s * 2 + 6, s * 2 + 6); }
    v.hits.push([x, y, key]);
    if (v.labels && (zoom >= 13 || (!ground && zoom >= 8)) && cs) {
      g.font = `700 9px "IBM Plex Mono", monospace`;
      const sub = ground ? "" : ` ${alt >= 18000 ? `FL${Math.round(alt / 100)}` : Math.round(alt / 100) * 100}`;
      const label = `${cs}${sub}`, tw = g.measureText(label).width + 6;
      if (room(x + s, y - 6, tw, 12) || key === sel) {
        g.fillStyle = cssVar("--panel"); g.globalAlpha = 0.9; g.fillRect(x + s, y - 6, tw, 12); g.globalAlpha = 1;
        g.fillStyle = ink; g.fillText(label, x + s + 3, y);
      }
    }
  });
}

// The card of the aircraft tapped: at the bottom, above the corner notes
function drawSelCard(g, c, w, h, ink) {
  const dash = (x) => (x == null || x === "" ? "" : x);
  const pick = (...xs) => xs.filter(Boolean).join("  ");
  const lines = [
    pick(c.cs, c.type, c.reg && c.reg !== c.cs ? c.reg : ""),
    pick(c.from || c.to ? `${dash(c.from) || "?"}→${dash(c.to) || "?"}` : "", c.sid ? `SID ${c.sid}` : "", c.star ? `STAR ${c.star}` : "", c.dep ? `RWY ${c.dep}` : c.arr ? `RWY ${c.arr}` : ""),
    pick(c.ground ? "ON GROUND" : `${Math.round(c.alt).toLocaleString("en")} ft`, `${Math.round(c.gs)} kt`, `HDG ${String(Math.round(c.hdg)).padStart(3, "0")}`),
    c.status || c.state || "",
    ...(Array.isArray(c.details) ? c.details.slice(0, 2) : []),
    c.freq ? `${c.freqName || "Freq"} ${c.freq}` : "",
    c.route && c.route.to ? `Taxi: ${c.route.via && c.route.via.length ? `via ${c.route.via.join(" ")} ` : ""}to ${c.route.to}` : c.note || "",
  ].filter(Boolean);
  g.font = `600 10px "IBM Plex Mono", monospace`;
  const maxW = w - 32;
  const wrapped = [];
  lines.forEach((text, i) => {
    let line = "";
    text.split(/\s+/).forEach((word) => { const next = line ? `${line} ${word}` : word; if (line && g.measureText(next).width > maxW) { wrapped.push([line, i]); line = word; } else line = next; });
    if (line) wrapped.push([line, i]);
  });
  const shown = wrapped.slice(0, Math.max(2, Math.floor((h - 60) / 13)));
  const ch = shown.length * 13 + 10, y = h - 30 - ch;
  g.fillStyle = ink; g.fillRect(10, y + 2, w - 16, ch);
  g.fillStyle = cssVar("--panel"); g.fillRect(8, y, w - 16, ch);
  g.strokeStyle = ink; g.lineWidth = 2; g.strokeRect(8, y, w - 16, ch);
  g.textBaseline = "top";
  shown.forEach(([l, i], k) => {
    g.font = i === 0 ? `700 11px "Pixelify Sans", "IBM Plex Mono", monospace` : `600 10px "IBM Plex Mono", monospace`;
    g.fillStyle = i === 3 ? cssVar("--coral-dark") : ink;
    g.fillText(l, 14, y + 6 + k * 13);
  });
  // A close mark, top right
  g.fillStyle = ink; g.font = `700 11px "IBM Plex Mono", monospace`; g.fillText("×", w - 22, y + 4);
}

// The pushback: a coral dashed way from the gate, arrows the way the aircraft goes (tail first),
// the aircraft where it stops (nose the way ATC said), and a card: PUSH · FACE W · TAIL L
function drawPush(g, p, at, ink, w, h) {
  if (Array.isArray(p.pts) && p.pts.length > 1) {
    const pts = p.pts.map(([la, lo]) => at(la, lo));
    g.lineJoin = "round"; g.lineCap = "round";
    g.beginPath(); pts.forEach(([x, y], i) => (i ? g.lineTo(x, y) : g.moveTo(x, y)));
    g.strokeStyle = ink; g.lineWidth = 8; g.stroke();
    g.setLineDash([8, 5]); g.strokeStyle = cssVar("--coral"); g.lineWidth = 5; g.stroke(); g.setLineDash([]);
    g.fillStyle = ink;
    for (let i = 1; i < pts.length; i++) {
      const [x1, y1] = pts[i - 1], [x2, y2] = pts[i];
      if (Math.hypot(x2 - x1, y2 - y1) < 14) continue;
      g.save(); g.translate((x1 + x2) / 2, (y1 + y2) / 2); g.rotate(Math.atan2(y2 - y1, x2 - x1));
      g.beginPath(); g.moveTo(4, 0); g.lineTo(-3, -4); g.lineTo(-3, 4); g.closePath(); g.fill();
      g.restore();
    }
  }
  if (Array.isArray(p.ghost)) {
    const [x, y] = at(p.ghost[0], p.ghost[1]);
    g.save(); g.translate(x, y); g.rotate(((p.ghost[2] || 0) * Math.PI) / 180);
    g.globalAlpha = 0.75;
    planeShape(g, 18, "M");
    g.fillStyle = cssVar("--coral"); g.strokeStyle = ink; g.lineWidth = 1.5; g.fill(); g.stroke();
    g.restore();
  }
  // The card, at the bottom
  const lines = [p.label || "PUSH", p.note || (p.gate ? `From gate ${p.gate}` : "")].filter(Boolean);
  g.font = `600 10px "IBM Plex Mono", monospace`;
  const ch = lines.length * 14 + 8, y = h - 30 - ch;
  g.fillStyle = ink; g.fillRect(10, y + 2, w - 16, ch);
  g.fillStyle = cssVar("--panel"); g.fillRect(8, y, w - 16, ch);
  g.strokeStyle = ink; g.lineWidth = 2; g.strokeRect(8, y, w - 16, ch);
  g.textBaseline = "top";
  lines.forEach((l, i) => {
    g.font = i ? `600 10px "IBM Plex Mono", monospace` : `700 11px "Pixelify Sans", "IBM Plex Mono", monospace`;
    g.fillStyle = i ? ink : cssVar("--coral-dark");
    g.fillText(l, 14, y + 5 + i * 14);
  });
}

// Draw one map element (a button on the board, in the inspector, or in the phone preview)
function drawMap(el, id) {
  const d = LIVE_DATA[id];
  const cv = el.querySelector("canvas");
  if (!d || !cv) return;
  const w = el.clientWidth, h = el.clientHeight;
  if (!w || !h) return;
  const dpr = window.devicePixelRatio || 1;
  if (cv.width !== Math.round(w * dpr) || cv.height !== Math.round(h * dpr)) { cv.width = Math.round(w * dpr); cv.height = Math.round(h * dpr); }
  const g = cv.getContext("2d");
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  const v = mapView(id);
  // Where the map looks: explore mode's own view, else the aircraft (or the whole route)
  let zoom, center;
  if (v.explore && v.center) { zoom = v.zoom != null ? v.zoom : MAP_ZOOM.Medium; center = v.center; }
  // On the ground with a taxi route, the map shows the route
  else if (v.zoom == null && taxiPts(d)) { const f = fitTaxi(d, w, h); zoom = f.zoom; center = f.center; }
  else if (v.zoom == null && d.zoom === "Whole route") { const f = fitZoom(d, w, h); zoom = f.zoom; center = f.center; }
  else { zoom = v.zoom != null ? v.zoom : MAP_ZOOM[d.zoom] || MAP_ZOOM.Medium; center = d.plane ? [d.plane.lat, d.plane.lon] : null; }
  if (!center) center = (d.route && d.route.points[0] && [d.route.points[0].lat, d.route.points[0].lon]) || [20, 0];
  v.shown = { zoom, center };
  const [cx, cy] = merc(center[0], center[1], zoom);
  const at = (lat, lon) => { const [x, y] = merc(lat, lon, zoom); return [x - cx + w / 2, y - cy + h / 2]; };
  const ink = cssVar("--ink");
  // Background
  if (d.bg === "osm" && d.tiles) {
    g.fillStyle = cssVar("--panel");
    g.fillRect(0, 0, w, h);
    const z = Math.max(0, Math.min(19, Math.round(zoom))), n = 2 ** z;
    const x0 = Math.floor((cx - w / 2) / 256), x1 = Math.floor((cx + w / 2) / 256), y0 = Math.floor((cy - h / 2) / 256), y1 = Math.floor((cy + h / 2) / 256);
    for (let ty = Math.max(0, y0); ty <= Math.min(n - 1, y1); ty++) for (let tx = x0; tx <= x1; tx++) {
      const img = tileImg(d.tiles, z, ((tx % n) + n) % n, ty, () => drawMap(el, id));
      if (img) g.drawImage(img, tx * 256 - cx + w / 2, ty * 256 - cy + h / 2, 256, 256);
    }
  } else {
    g.fillStyle = cssVar("--sky");
    g.globalAlpha = 0.35;
    g.fillRect(0, 0, w, h);
    g.globalAlpha = 1;
    const [la1, lo1] = unmerc(cx - w / 2, cy + h / 2, zoom), [la2, lo2] = unmerc(cx + w / 2, cy - h / 2, zoom);
    const inView = (s) => s.maxX >= lo1 && s.minX <= lo2 && s.maxY >= la1 && s.minY <= la2;
    const path = (s, close) => {
      g.beginPath();
      for (let i = 0; i < s.pts.length; i += 2) { const [x, y] = at(s.pts[i + 1], s.pts[i]); if (i) g.lineTo(x, y); else g.moveTo(x, y); }
      if (close) g.closePath();
    };
    const W = worldShapes();
    // The lat/lon grid, faint, under the land
    const step = gridStep(zoom);
    g.strokeStyle = cssVar("--sky-dark");
    g.globalAlpha = 0.25;
    g.lineWidth = 1;
    g.beginPath();
    for (let lo = Math.ceil(lo1 / step) * step, k = 0; lo <= lo2 && k < 200; lo += step, k++) { const x = Math.round(at(0, lo)[0]) + 0.5; g.moveTo(x, 0); g.lineTo(x, h); }
    for (let la = Math.ceil(la1 / step) * step, k = 0; la <= la2 && k < 200; la += step, k++) { const y = Math.round(at(la, 0)[1]) + 0.5; g.moveTo(0, y); g.lineTo(w, y); }
    g.stroke();
    g.globalAlpha = 1;
    g.fillStyle = cssVar("--tile");
    g.strokeStyle = ink;
    g.lineWidth = 1.5;
    W.land.filter(inView).forEach((s) => { path(s, true); g.fill("evenodd"); g.stroke(); });
    // Lakes and rivers
    g.fillStyle = cssVar("--sky");
    g.strokeStyle = cssVar("--sky-dark");
    g.lineWidth = 1;
    g.globalAlpha = 0.6;
    W.lakes.filter(inView).forEach((s) => { path(s, true); g.fill("evenodd"); });
    g.globalAlpha = 1;
    W.lakes.filter(inView).forEach((s) => { path(s, true); g.stroke(); });
    if (zoom >= 4) { g.lineWidth = zoom >= 7 ? 1.5 : 1; W.rivers.filter(inView).forEach((s) => { path(s, false); g.stroke(); }); }
    // State and province lines, then country borders
    g.strokeStyle = cssVar("--rule");
    g.lineWidth = 1;
    if (zoom >= 4) { g.setLineDash([2, 3]); W.states.filter(inView).forEach((s) => { path(s, false); g.stroke(); }); }
    g.strokeStyle = cssVar("--slate");
    g.globalAlpha = 0.7;
    g.setLineDash([5, 3]);
    W.borders.filter(inView).forEach((s) => { path(s, false); g.stroke(); });
    g.setLineDash([]);
    g.globalAlpha = 1;
    // Grid labels along the left and top edges
    g.font = `500 9px "IBM Plex Mono", monospace`;
    g.fillStyle = cssVar("--slate");
    g.textBaseline = "top";
    const deg = (v, pos, neg) => `${Math.abs(+v.toFixed(2))}°${v > 0 ? pos : v < 0 ? neg : ""}`;
    for (let la = Math.ceil(la1 / step) * step, k = 0; la <= la2 && k < 200; la += step, k++) { const y = at(la, 0)[1]; if (y > 12 && y < h - 12) g.fillText(deg(la, "N", "S"), 3, y + 2); }
    for (let lo = Math.ceil(lo1 / step) * step, k = 0; lo <= lo2 && k < 200; lo += step, k++) { const x = at(0, lo)[0]; if (x > 30 && x < w - 30) g.fillText(deg(((lo + 540) % 360) - 180, "E", "W"), x + 2, 3); }
  }
  // Route labels come first; cities and airports only label where there's room left
  const room = labelSpace();
  const pts = (d.route && d.route.points) || [];
  g.font = `600 11px "Pixelify Sans", "IBM Plex Mono", monospace`;
  let lastLabel = null;
  const routeLabels = new Set();
  pts.forEach((p, i) => {
    const [x, y] = at(p.lat, p.lon);
    if (x < -20 || y < -20 || x > w + 20 || y > h + 20) return;
    const big = p.k === "dep" || p.k === "dest";
    if (big || !lastLabel || Math.hypot(x - lastLabel[0], y - lastLabel[1]) > 46) {
      if (room(x + 6, y - 7, g.measureText(p.n).width + 6, 14) || big) { routeLabels.add(i); lastLabel = [x, y]; }
    }
  });
  const view = [...unmerc(cx - w / 2, cy + h / 2, zoom), ...unmerc(cx + w / 2, cy - h / 2, zoom)];
  drawAirports(g, d, zoom, w, h, at, room, ink, view, id);
  if (taxiPts(d)) drawTaxi(g, d.taxi, at, zoom, room, ink, d.taxi.src === "atc" ? cssVar("--mint") : cssVar("--sun"));
  // The tapped aircraft's taxi route, in lilac
  if (d.sel && d.sel.route && Array.isArray(d.sel.route.pts) && d.sel.route.pts.length > 1) drawTaxi(g, d.sel.route, at, zoom, room, ink, cssVar("--lilac"));
  if (d.bg !== "osm") drawPlaces(g, d, zoom, w, h, at, room, ink);
  // The SimBrief route: lilac dashes under the EFB's
  if (d.ofp && Array.isArray(d.ofp.pts) && d.ofp.pts.length > 1) {
    g.beginPath();
    d.ofp.pts.forEach(([, la, lo], i) => { const [x, y] = at(la, lo); if (i) g.lineTo(x, y); else g.moveTo(x, y); });
    g.setLineDash([7, 5]); g.strokeStyle = cssVar("--lilac"); g.lineWidth = 3; g.stroke(); g.setLineDash([]);
  }
  // The trail flown: a thin coral line up to the aircraft
  if (d.trail && d.trail.length > 1) {
    g.beginPath();
    d.trail.forEach(([la, lo], i) => { const [x, y] = at(la, lo); if (i) g.lineTo(x, y); else g.moveTo(x, y); });
    if (d.plane) { const [x, y] = at(d.plane.lat, d.plane.lon); g.lineTo(x, y); }
    g.strokeStyle = cssVar("--coral");
    g.lineWidth = 2;
    g.stroke();
  }
  // The route: a mint line with ink edges, square fixes, names where there's room
  if (pts.length > 1) {
    g.beginPath();
    pts.forEach((p, i) => { const [x, y] = at(p.lat, p.lon); if (i) g.lineTo(x, y); else g.moveTo(x, y); });
    g.strokeStyle = ink; g.lineWidth = 5; g.stroke();
    g.strokeStyle = cssVar("--mint"); g.lineWidth = 3; g.stroke();
  }
  g.font = `600 11px "Pixelify Sans", "IBM Plex Mono", monospace`;
  g.textBaseline = "middle";
  pts.forEach((p, i) => {
    const [x, y] = at(p.lat, p.lon);
    if (x < -20 || y < -20 || x > w + 20 || y > h + 20) return;
    const big = p.k === "dep" || p.k === "dest";
    const s = big ? 8 : 6;
    g.fillStyle = big ? cssVar("--sun") : cssVar("--tile");
    g.fillRect(x - s / 2, y - s / 2, s, s);
    g.strokeStyle = ink; g.lineWidth = 2; g.strokeRect(x - s / 2, y - s / 2, s, s);
    if (routeLabels.has(i)) {
      const tw = g.measureText(p.n).width;
      g.fillStyle = cssVar("--panel"); g.fillRect(x + 6, y - 7, tw + 6, 14);
      g.strokeStyle = ink; g.lineWidth = 1; g.strokeRect(x + 6, y - 7, tw + 6, 14);
      g.fillStyle = ink; g.fillText(p.n, x + 9, y);
    }
  });
  drawTraffic(g, d, v, at, zoom, w, h, room, ink);
  // The taxi card; or, with no route to draw, why (no EFB route yet, say), at the top
  if (d.taxi && (d.taxi.note || taxiPts(d)) && h >= 70) drawTaxiCard(g, d.taxi, w, h, false, ink);
  else if (pts.length < 2 && d.info && !d.status && h >= 70) drawInfo(g, d.info, w, ink);
  if (d.sel && d.sel.k && h >= 90) drawSelCard(g, d.sel, w, h, ink);
  // The pushback ATC gave, once the aircraft is tapped: the way, where it stops, and its card
  const push = d.push && d.push.label ? d.push : null;
  if (push && v.push) drawPush(g, push, at, ink, w, h);
  // The aircraft: an ink arrow pointing along the true heading, with a white edge
  v.me = null;
  if (d.plane) {
    const [x, y] = at(d.plane.lat, d.plane.lon);
    v.me = [x, y];
    // A pushback to see: a ring that beats round the aircraft until it's tapped
    if (push && !v.push) {
      const r = 15 + 3 * Math.sin(Date.now() / 220);
      g.beginPath(); g.arc(x, y, r, 0, Math.PI * 2);
      g.strokeStyle = ink; g.lineWidth = 5; g.stroke();
      g.strokeStyle = cssVar("--coral"); g.lineWidth = 3; g.stroke();
    }
    g.save();
    g.translate(x, y);
    g.rotate(((d.plane.hdg || 0) * Math.PI) / 180);
    g.beginPath();
    [[0, -11], [8, 8], [0, 4], [-8, 8]].forEach(([px, py], i) => (i ? g.lineTo(px, py) : g.moveTo(px, py)));
    g.closePath();
    g.fillStyle = ink; g.strokeStyle = "#ffffff"; g.lineWidth = 2;
    g.stroke(); g.fill();
    g.restore();
  }
  // Corner notes: speed and altitude, or the sim's state; the map credit for street maps
  const note = el.querySelector(".lmap-note");
  if (note) {
    const p = d.plane;
    note.textContent = d.status ? d.status : p ? [p.gs != null ? `${Math.round(p.gs)} kt` : "", p.alt != null ? `${Math.round(p.alt).toLocaleString("en")} ft` : ""].filter(Boolean).join(" · ") : "No aircraft yet";
  }
  const credit = el.querySelector(".lmap-credit");
  if (credit) credit.hidden = d.bg !== "osm";
  el.classList.toggle("free", !!v.explore);
  el.classList.toggle("has-taxi", !!taxiPts(d));
  el.classList.toggle("no-traffic", !v.traffic);
  el.classList.toggle("no-labels", !v.labels);
}

// The map's markup inside a button; drawMap fills the canvas
function mapHTML(id) {
  return `<span class="lmap" data-map="${esc(id)}"><canvas aria-hidden="true"></canvas><span class="lmap-note"></span><span class="lmap-credit" hidden>© OpenStreetMap</span>`
    + `<span class="lmap-ctl" role="group" aria-label="Map controls"><button type="button" data-z="1" aria-label="Zoom in">+</button><button type="button" data-z="-1" aria-label="Zoom out">−</button><button type="button" data-follow="1" aria-label="Follow the aircraft">◎</button><button type="button" data-taxi="1" aria-label="Show the taxi route">TAXI</button></span>`
    + `<span class="lmap-ctl lmap-ctl-r" role="group" aria-label="Map layers"><button type="button" data-traffic="1" aria-label="Show other aircraft">✈</button><button type="button" data-labels="1" aria-label="Show callsigns">ABC</button></span></span>`;
}
// A screen's markup inside a button: the map, the flight plan or the radio messages
function screenHTML(id) {
  const d = LIVE_DATA[id] || {};
  return d.kind === "ofp" ? ofpHTML(id) : d.kind === "atc" ? atcHTML(id) : d.kind === "gsx" ? gsxHTML(id)
    : d.kind === "spotify" ? spotifyHTML(id) : d.kind === "spotify_list" ? spotifyListHTML(id) : mapHTML(id);
}
// Draw every element showing this screen: maps on their canvas, text screens in place
// Many updates can come between two frames (the sim, traffic, a screen's data): each map is drawn
// once per frame, with the newest of them
const DRAW_DUE = new Set();
function drawMaps(id) {
  if (DRAW_DUE.has(id)) return;
  DRAW_DUE.add(id);
  requestAnimationFrame(() => { DRAW_DUE.delete(id); drawMapsNow(id); });
}
function drawMapsNow(id) {
  const q = CSS.escape(id);
  document.querySelectorAll(`.lmap[data-map="${q}"]`).forEach((el) => drawMap(el, id));
  document.querySelectorAll(`.lscr[data-scr="${q}"]`).forEach((el) => drawTextScreen(el, id));
}

// ----- Text screens: the SimBrief flight plan, and BeyondATC's radio messages -----
//   {"kind":"ofp", rev, title, text, marks: [[name, line]], pdf, note, from, to, depRwy, arrRwy}
//   {"kind":"atc", rev, lines: [[from, text, "atc"|"me"|"traffic"]], station, comms, status}
function ofpHTML(id) {
  return `<span class="lscr lofp" data-scr="${esc(id)}"><span class="lscr-bar"><b class="lscr-t"></b><span class="lscr-tabs" role="group" aria-label="Sections"></span>`
    + `<button type="button" data-ofp="reload" aria-label="Get the latest OFP from SimBrief">↻</button><button type="button" data-ofp="pdf" aria-label="Open the OFP as a PDF">PDF</button></span>`
    + `<pre class="lscr-body" tabindex="0"></pre><span class="lscr-note"></span></span>`;
}
function atcHTML(id) {
  return `<span class="lscr latc" data-scr="${esc(id)}"><span class="lscr-bar"><b class="lscr-t">ATC</b><span class="latc-st"></span></span>`
    + `<span class="lscr-body latc-list" role="log" aria-live="polite" tabindex="0"></span><span class="lscr-note"></span></span>`;
}
//   {"kind":"gsx", rev, up, shown, title, sub, entries: [[label, disabled]], services: [[id, name,
//    look 0 can ask|1 under way|2 done|3 not now]], place, state, msg, push "FACE W · TAIL L", status}
// A tap sends the plugin {pick, label}, {open}, {close}, {service} or {push}.
function gsxHTML(id) {
  return `<span class="lscr lgsx" data-scr="${esc(id)}"><span class="lscr-bar"><b class="lscr-t">GSX</b><span class="lgsx-sub"></span>`
    + `<button type="button" class="push" data-gsx="push" aria-label="Ask GSX for the pushback">PUSH</button><button type="button" data-gsx="menu" aria-label="Open the GSX menu">MENU</button></span>`
    + `<span class="lscr-body lgsx-list" tabindex="0"></span><span class="lscr-note"></span></span>`;
}
// Runways, frequencies and the taxiways after "via", picked out
function atcMarkup(text) {
  let h = esc(text);
  h = h.replace(/\b(runway|rwy)\s+(\d{1,2}[LRC]?)\b/gi, '<mark class="rw">$1 $2</mark>');
  h = h.replace(/\b(1[1-3]\d\.\d{1,3})\b/g, '<mark class="fq">$1</mark>');
  h = h.replace(/\bvia ([A-Z0-9, ]+?)(?=\.|$| hold| cross)/g, (m, list) => `via <mark class="tw">${list}</mark>`);
  return h;
}
function drawTextScreen(box, id) {
  const d = LIVE_DATA[id];
  if (!d) return;
  if (d.kind === "spotify") { drawSpotify(box, d); return; }
  if (d.kind === "spotify_list") { drawSpotifyList(box, d); return; }
  const note = box.querySelector(".lscr-note");
  if (d.kind === "ofp") {
    if (box.dataset.rev === String(d.rev)) return;
    box.dataset.rev = String(d.rev);
    box.querySelector(".lscr-t").textContent = d.title || "SimBrief OFP";
    const tabs = box.querySelector(".lscr-tabs");
    tabs.innerHTML = "";
    (d.marks || []).forEach(([name, line]) => {
      const b = document.createElement("button");
      b.type = "button";
      b.textContent = name;
      b.dataset.line = line;
      tabs.appendChild(b);
    });
    const body = box.querySelector(".lscr-body"), top = body.scrollTop;
    body.textContent = d.text || "";
    body.scrollTop = top;
    box.querySelector('[data-ofp="pdf"]').hidden = !d.pdf;
    note.textContent = d.note || "";
    note.hidden = !d.note || !!d.text && !/fetch|set your|couldn|simbrief:/i.test(d.note);
    return;
  }
  if (d.kind === "atc") {
    if (box.dataset.rev === `${d.rev}|${d.station}|${d.comms}|${d.status}`) return;
    box.dataset.rev = `${d.rev}|${d.station}|${d.comms}|${d.status}`;
    box.querySelector(".latc-st").textContent = [d.station, d.comms && d.comms !== "ready" ? d.comms : ""].filter(Boolean).join(" · ");
    const list = box.querySelector(".latc-list");
    list.innerHTML = (d.lines || []).slice().reverse().map(([from, text, kind]) => `<span class="latc-l ${esc(kind || "atc")}"><b>${esc(from)}</b> ${atcMarkup(text)}</span>`).join("");
    note.textContent = d.status || (d.lines && d.lines.length ? "" : "No radio messages yet.");
    note.hidden = !note.textContent;
    return;
  }
  if (d.kind === "gsx") {
    if (box.dataset.rev === `${d.rev}|${d.push}`) return;
    box.dataset.rev = `${d.rev}|${d.push}`;
    const open = d.up && d.shown && (d.entries || []).some(([l]) => l);
    box.querySelector(".lscr-t").textContent = open ? d.title || "GSX" : "GSX";
    box.querySelector(".lgsx-sub").textContent = open ? d.sub || "" : [d.place, d.state].filter(Boolean).join(" · ");
    const push = box.querySelector('[data-gsx="push"]');
    push.textContent = d.push ? `PUSH ${d.push.replace("FACE ", "").replace(" · TAIL ", " ")}` : "PUSH";
    push.classList.toggle("atc", !!d.push);
    push.disabled = !d.up;
    const menu = box.querySelector('[data-gsx="menu"]');
    menu.textContent = open ? "✕" : "MENU";
    menu.dataset.open = open ? "1" : "";
    menu.disabled = !d.up;
    const list = box.querySelector(".lgsx-list");
    list.classList.toggle("svc", !open);
    list.innerHTML = !d.up ? "" : open
      ? d.entries.map(([label, off], i) => (label ? `<button type="button" class="lgsx-o" data-pick="${i}" data-label="${esc(label)}"${off ? " disabled" : ""}>${esc(label)}</button>` : "")).join("")
      : (d.services || []).map(([sid, name, look]) => `<button type="button" class="lgsx-o s${look}" data-svc="${esc(sid)}"${look === 0 ? "" : " disabled"}>${esc(name)}</button>`).join("");
    if (open) list.scrollTop = 0;
    note.textContent = d.status || d.msg || "";
    note.hidden = !note.textContent;
  }
}

// A map is a screen: drag it to move (it stops following the aircraft), the wheel or + and - to
// zoom, the target to follow again; a tap on an aircraft opens its card, a tap elsewhere closes it.
// Clicks on a screen never press or select its button (double-click still opens Edit button).
(() => {
  let drag = null;
  const mapOf = (target) => target.closest && target.closest(".lmap");
  const zoomBy = (id, by) => {
    const v = mapView(id), s = v.shown || { zoom: MAP_ZOOM.Medium };
    v.zoom = Math.max(2, Math.min(18, (v.zoom != null ? v.zoom : s.zoom) + by));
    if (v.explore && !v.center && s.center) v.center = s.center;
    drawMaps(id);
  };
  const tap = (el, id, e) => {
    const v = mapView(id), r = el.getBoundingClientRect(), x = e.clientX - r.left, y = e.clientY - r.top;
    const d = LIVE_DATA[id] || {};
    // The card's close mark
    if (d.sel && d.sel.k && x > r.width - 30 && y > r.height * 0.4) { window.mapLooksAt && window.mapLooksAt(id, { select: null }); d.sel = null; drawMaps(id); return; }
    // Our aircraft, with a pushback to see: show it (or hide it again), the whole way in view
    if (d.push && d.push.label && v.me && Math.hypot(v.me[0] - x, v.me[1] - y) < 22) {
      v.push = !v.push;
      const pts = v.push && Array.isArray(d.push.pts) ? d.push.pts : null;
      if (pts && pts.length > 1) {
        const f = fitTaxi({ taxi: { pts } }, r.width, r.height);
        Object.assign(v, { explore: true, zoom: Math.min(17, f.zoom), center: f.center });
      }
      drawMaps(id);
      return;
    }
    let best = null, bd = 18;
    (v.hits || []).forEach(([hx, hy, key]) => { const dist = Math.hypot(hx - x, hy - y); if (dist < bd) { bd = dist; best = key; } });
    if (best || (d.sel && d.sel.k)) {
      if (window.mapLooksAt) window.mapLooksAt(id, { select: best });
      if (!best) d.sel = null;
      drawMaps(id);
    }
  };
  document.addEventListener("pointerdown", (e) => {
    const el = mapOf(e.target);
    if (!el || e.button !== 0 || e.target.closest(".lmap-ctl")) return;
    const id = el.dataset.map, v = mapView(id), s = v.shown || { zoom: MAP_ZOOM.Medium, center: [0, 0] };
    drag = { el, id, x: e.clientX, y: e.clientY, center: v.center || s.center, zoom: v.zoom != null ? v.zoom : s.zoom, moved: false };
    e.stopPropagation();
  }, true);
  document.addEventListener("pointermove", (e) => {
    if (!drag) return;
    if (!drag.moved && Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < 5) return;
    drag.moved = true;
    const v = mapView(drag.id);
    const [x, y] = merc(drag.center[0], drag.center[1], drag.zoom);
    Object.assign(v, { explore: true, zoom: drag.zoom, center: unmerc(x - (e.clientX - drag.x), y - (e.clientY - drag.y), drag.zoom) });
    drawMaps(drag.id);
  });
  document.addEventListener("pointerup", (e) => {
    if (drag && !drag.moved) tap(drag.el, drag.id, e);
    if (drag) drag.done = Date.now();
    drag = null;
  });
  document.addEventListener("click", (e) => {
    const scr = e.target.closest && e.target.closest(".lmap, .lscr");
    if (!scr) return;
    e.preventDefault(); e.stopPropagation();
    if (scr.classList.contains("lscr")) { screenClick(scr, e); return; }
    const id = scr.dataset.map, v = mapView(id);
    const zb = e.target.closest("[data-z]"), fb = e.target.closest("[data-follow]"), tb = e.target.closest("[data-taxi]");
    const trb = e.target.closest("[data-traffic]"), lb = e.target.closest("[data-labels]");
    if (tb && taxiPts(LIVE_DATA[id] || {})) {
      const f = fitTaxi(LIVE_DATA[id], scr.clientWidth, scr.clientHeight);
      Object.assign(v, { explore: true, zoom: f.zoom, center: f.center });
      drawMaps(id);
    } else if (zb) zoomBy(id, Number(zb.dataset.z));
    else if (fb) { v.center = null; v.explore = false; drawMaps(id); }
    else if (trb) { v.traffic = !v.traffic; drawMaps(id); }
    else if (lb) { v.labels = !v.labels; drawMaps(id); }
  }, true);
  document.addEventListener("wheel", (e) => {
    const el = mapOf(e.target);
    if (el) { e.preventDefault(); zoomBy(el.dataset.map, e.deltaY < 0 ? 1 : -1); return; }
    // A text screen scrolls itself; the page doesn't
    if (e.target.closest && e.target.closest(".lscr")) e.stopPropagation();
  }, { passive: false });
  // The OFP's section tabs, Reload and PDF; GSX's options, services, PUSH and MENU
  function screenClick(scr, e) {
    const id = scr.dataset.scr, d = LIVE_DATA[id] || {};
    if (d.kind === "spotify" || d.kind === "spotify_list") { spotifyClick(scr, id, d, e); return; }
    const tab = e.target.closest("[data-line]"), act = e.target.closest("[data-ofp]");
    const gsx = e.target.closest("[data-pick], [data-svc], [data-gsx]");
    if (gsx && !gsx.disabled && window.mapLooksAt) {
      if (gsx.dataset.pick != null) window.mapLooksAt(id, { pick: Number(gsx.dataset.pick), label: gsx.dataset.label });
      else if (gsx.dataset.svc) window.mapLooksAt(id, { service: gsx.dataset.svc });
      else if (gsx.dataset.gsx === "push") { window.mapLooksAt(id, { push: true }); say("Asking GSX for the pushback"); }
      else if (gsx.dataset.gsx === "menu") window.mapLooksAt(id, gsx.dataset.open ? { close: true } : { open: true });
      return;
    }
    if (tab) {
      const body = scr.querySelector(".lscr-body");
      const lh = parseFloat(getComputedStyle(body).lineHeight) || 13;
      body.scrollTop = Number(tab.dataset.line) * lh;
    } else if (act && act.dataset.ofp === "reload") {
      if (window.mapLooksAt) window.mapLooksAt(id, { reload: true });
      say("Getting your latest OFP from SimBrief");
    } else if (act && act.dataset.ofp === "pdf" && d.pdf) {
      backend.run("run_button", { steps: [{ kind: "url", fields: { url: d.pdf } }], button: { id: "ofp-pdf", state: 0 } }).catch((err) => say(String(err)));
    }
  }
  // Aircraft glide between updates: maps with traffic redraw a few times a second; a song's
  // progress bar moves on
  setInterval(() => {
    Object.keys(LIVE_DATA).forEach((id) => {
      const d = LIVE_DATA[id];
      if (d && d.kind === "spotify" && d.playing) drawMaps(id);
      // (and a pushback's ring beats)
      if (d && d.kind === "map" && ((Array.isArray(d.traffic) && d.traffic.length && mapView(id).traffic) || (d.push && d.push.label && !mapView(id).push))) drawMaps(id);
    });
  }, 150);
  // Maps redraw at their new size when the window changes
  window.addEventListener("resize", () => Object.keys(LIVE_DATA).forEach(drawMaps));
})();

// ----- Spotify (the minnion_spotify plugin) -----
//   {"kind":"spotify", rev, state "ok"|"signin"|"idle"|"offline", msg, title, artist, album,
//    cover (the cover as a data: URL, "" for none), dur, pos, at, playing}
//   {"kind":"spotify_list", rev, tabs [[id, name]], tab, items [[id, title, sub, art 12 × 12]],
//    note, foot {shuffle, repeat} or null}
// Now playing: the cover fills the button; a dark strip over its bottom has the song, the artist,
// the time, the length and the progress bar. A tap plays or pauses; a tap on the progress bar
// seeks. A list's tap plays the row. Taps go to the plugin as {toggle}, {seek: ms}, {tab}, {play},
// {shuffle} or {repeat}.
function spotifyHTML(id) {
  return `<span class="lscr lspot" data-scr="${esc(id)}" role="img" aria-label="Spotify, now playing"><img class="lspot-cover" alt="" hidden>`
    + `<span class="lspot-none" aria-hidden="true"></span><span class="lspot-strip"><b class="lspot-t"></b><i class="lspot-a"></i>`
    + `<span class="lspot-times"><span class="lspot-pos"></span><span class="lspot-dur"></span></span>`
    + `<span class="lspot-bar"><span class="lspot-fill"></span></span></span></span>`;
}
function spotifyListHTML(id) {
  return `<span class="lscr lspl" data-scr="${esc(id)}"><span class="lscr-bar"><span class="lscr-tabs" role="tablist" aria-label="Lists"></span></span>`
    + `<span class="lscr-body lspl-list" tabindex="0"></span><span class="lscr-note"></span><span class="lscr-bar lspl-foot" hidden></span></span>`;
}
// A base64 RGB picture as bytes, kept by its text (list rows' small covers)
const SP_ART = new Map();
function spArt(b64) {
  if (!b64) return null;
  let px = SP_ART.get(b64);
  if (!px) {
    try { px = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)); } catch (e) { return null; }
    if (SP_ART.size > 200) SP_ART.clear();
    SP_ART.set(b64, px);
  }
  return px;
}
function spBlocks(g, px, x, y, side) {
  const n = Math.round(Math.sqrt(px.length / 3));
  if (!n) return;
  const cell = side / n;
  for (let r = 0; r < n; r++) for (let c = 0; c < n; c++) {
    const i = (r * n + c) * 3;
    g.fillStyle = `rgb(${px[i]},${px[i + 1]},${px[i + 2]})`;
    g.fillRect(Math.floor(x + c * cell), Math.floor(y + r * cell), Math.ceil(cell), Math.ceil(cell));
  }
}
const spTime = (ms) => { const s = Math.max(0, Math.floor(ms / 1000)); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`; };
// Where a song is now: from where it was, moving on while it plays
const spPos = (d) => Math.min(d.dur || 0, (d.pos || 0) + (d.playing && d.at ? Math.max(0, Date.now() - d.at) : 0));
function drawSpotify(box, d) {
  const img = box.querySelector(".lspot-cover");
  if (!img) return;
  // Only a picture given as data (the plugin sends the cover itself) is shown
  const cover = typeof d.cover === "string" && d.cover.startsWith("data:image/") ? d.cover : "";
  if (img.dataset.src !== cover) {
    img.dataset.src = cover;
    if (cover) img.src = cover; else img.removeAttribute("src");
    img.hidden = !cover;
  }
  box.classList.toggle("no-cover", !cover);
  const ok = d.state === "ok";
  box.classList.toggle("msg", !ok && d.state !== "offline");
  const title = ok || d.state === "offline" ? d.title || "" : d.msg || "";
  const artist = ok ? d.artist || "" : d.state === "offline" ? "Offline" : "";
  const t = box.querySelector(".lspot-t"), a = box.querySelector(".lspot-a");
  if (t.textContent !== title) t.textContent = title;
  if (a.textContent !== artist) a.textContent = artist;
  a.hidden = !artist;
  box.setAttribute("aria-label", title ? `Spotify, now playing: ${title}${artist ? `, ${artist}` : ""}` : "Spotify, now playing");
  const has = !!d.dur;
  box.querySelector(".lspot-times").hidden = !has;
  box.querySelector(".lspot-bar").hidden = !has;
  if (has) {
    const at = spPos(d);
    box.querySelector(".lspot-pos").textContent = spTime(at);
    box.querySelector(".lspot-dur").textContent = spTime(d.dur);
    box.querySelector(".lspot-fill").style.width = `${Math.min(100, (100 * at) / d.dur).toFixed(2)}%`;
  }
}
function drawSpotifyList(box, d) {
  if (box.dataset.rev === String(d.rev)) return;
  box.dataset.rev = String(d.rev);
  const tabs = box.querySelector(".lscr-tabs");
  tabs.innerHTML = (d.tabs || []).map(([tid, name]) => (d.tabs.length > 1
    ? `<button type="button" role="tab" data-sptab="${esc(tid)}" aria-selected="${tid === d.tab}" class="${tid === d.tab ? "on" : ""}">${esc(name)}</button>`
    : `<b class="lscr-t">${esc(name)}</b>`)).join("");
  const list = box.querySelector(".lspl-list");
  const keep = list.dataset.tab === d.tab ? list.scrollTop : 0;
  list.dataset.tab = d.tab || "";
  list.innerHTML = (d.items || []).map(([iid, title, sub, art]) => `<button type="button" class="lspl-o" data-spplay="${esc(iid)}"><canvas width="12" height="12" data-art="${esc(art || "")}" aria-hidden="true"></canvas>`
    + `<span><b>${esc(title)}</b><i>${esc(sub || "")}</i></span></button>`).join("");
  list.querySelectorAll("canvas[data-art]").forEach((c) => {
    const px = spArt(c.dataset.art), g = c.getContext("2d");
    if (px) spBlocks(g, px, 0, 0, 12);
    else { g.fillStyle = "#56627d"; g.fillRect(0, 0, 12, 12); }
  });
  list.scrollTop = keep;
  const note = box.querySelector(".lscr-note");
  note.textContent = d.note || "";
  note.hidden = !d.note;
  const foot = box.querySelector(".lspl-foot");
  foot.hidden = !d.foot;
  if (d.foot) {
    const rep = { off: "REPEAT OFF", context: "REPEAT ALL", track: "REPEAT ONE" }[d.foot.repeat] || "REPEAT";
    foot.innerHTML = `<button type="button" data-spfoot="shuffle" aria-pressed="${!!d.foot.shuffle}" class="${d.foot.shuffle ? "on" : ""}">SHUFFLE</button>`
      + `<button type="button" data-spfoot="repeat" class="${d.foot.repeat && d.foot.repeat !== "off" ? "on" : ""}">${rep}</button>`;
  }
}
function spotifyClick(scr, id, d, e) {
  const look = (data) => window.mapLooksAt && window.mapLooksAt(id, data);
  if (d.kind === "spotify") {
    // The progress bar (and a little round it, for a finger) seeks; anywhere else plays or pauses
    const bar = scr.querySelector(".lspot-bar"), b = bar && bar.getBoundingClientRect();
    if (d.dur && b && b.width && e.clientY >= b.top - 12 && e.clientY <= b.bottom + 12) look({ seek: Math.round(Math.max(0, Math.min(1, (e.clientX - b.left) / b.width)) * d.dur) });
    else look({ toggle: true });
    return;
  }
  const tab = e.target.closest("[data-sptab]"), row = e.target.closest("[data-spplay]"), foot = e.target.closest("[data-spfoot]");
  if (tab) look({ tab: tab.dataset.sptab });
  else if (row) look({ play: row.dataset.spplay });
  else if (foot) look({ [foot.dataset.spfoot]: true });
}
