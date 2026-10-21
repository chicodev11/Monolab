package com.github.chicodev11.monolab.addon;

/** The page friends see: a pannable, zoomable canvas over the tiles {@link MapWeb} serves. No backslashes in here. */
final class MapPage {
    private MapPage() {
    }

    static final String HTML = """
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, maximum-scale=1">
<title>World map</title>
<style>
:root {
  --bg: #111111; --panel: #1a1a1a; --raise: #272727; --line: #2e2e2e; --text: #f2f2f2; --dim: #a6a6a6;
  --accent: #f2f2f2; --ink: #111111; --shadow: 0 6px 24px rgba(0, 0, 0, .35);
  --ease: cubic-bezier(.2, .7, .3, 1);
}
@media (prefers-color-scheme: light) {
  :root { --bg: #f2f2f2; --panel: #ffffff; --raise: #ebebeb; --line: #dadada; --text: #161616; --dim: #5c5c5c; --accent: #161616; --ink: #ffffff; --shadow: 0 6px 24px rgba(0, 0, 0, .14); }
}
* { box-sizing: border-box; }
html, body { margin: 0; height: 100%; overflow: hidden; background: var(--bg); color: var(--text);
  font: 14px/1.4 Inter, system-ui, -apple-system, "Segoe UI", "Noto Sans KR", sans-serif; -webkit-font-smoothing: antialiased; }
canvas { position: fixed; inset: 0; width: 100%; height: 100%; touch-action: none; cursor: grab; outline: none; }
canvas.drag { cursor: grabbing; }
.bar { position: fixed; z-index: 3; display: flex; gap: 8px; align-items: center; }
.top-left { top: 14px; left: 14px; }
.top-right { top: 14px; right: 14px; }
.bottom-left { bottom: 14px; left: 14px; }
.bottom-right { bottom: 14px; right: 14px; flex-direction: column; align-items: flex-end; }
.pill { display: flex; align-items: center; gap: 8px; height: 38px; padding: 0 14px; border-radius: 12px; background: var(--panel);
  border: 1px solid var(--line); box-shadow: var(--shadow); animation: rise .45s var(--ease) both; }
.pill strong { font-weight: 650; letter-spacing: -.01em; }
.seg { display: flex; padding: 3px; gap: 2px; height: 38px; border-radius: 12px; background: var(--panel); border: 1px solid var(--line);
  box-shadow: var(--shadow); animation: rise .45s var(--ease) .05s both; }
.seg button { position: relative; border: 0; background: transparent; color: var(--dim); font: inherit; font-weight: 550; padding: 0 12px;
  border-radius: 9px; cursor: pointer; transition: color .15s, background .15s, transform .15s var(--ease); }
.seg button:hover { color: var(--text); background: var(--raise); }
.seg button:active { transform: scale(.96); }
.seg button.on { background: var(--accent); color: var(--ink); }
.icon { width: 38px; height: 38px; display: grid; place-items: center; border-radius: 12px; border: 1px solid var(--line); background: var(--panel);
  color: var(--text); font: inherit; font-size: 19px; font-weight: 500; cursor: pointer; box-shadow: var(--shadow);
  transition: background .15s, transform .15s var(--ease), color .15s; animation: rise .45s var(--ease) .1s both; }
.icon:hover { background: var(--raise); }
.icon:active { transform: scale(.92); }
.icon.on { background: var(--accent); color: var(--ink); border-color: transparent; }
.coords { height: 32px; padding: 0 12px; font-variant-numeric: tabular-nums; color: var(--dim); font-size: 13px; }
.coords b { color: var(--text); font-weight: 600; }
.players { min-width: 190px; max-width: 260px; border-radius: 14px; background: var(--panel); border: 1px solid var(--line); box-shadow: var(--shadow);
  padding: 6px; display: grid; gap: 2px; animation: rise .35s var(--ease) both; }
.players[hidden] { display: none; }
.players p { margin: 0 8px 6px; color: var(--dim); font-size: 13px; }
.players img, .players .face { width: 22px; height: 22px; flex: none; border-radius: 5px; image-rendering: pixelated; background: var(--raise); }
.players .face { display: grid; place-items: center; color: var(--dim); font-size: 11px; font-weight: 600; }
.players h4 { margin: 4px 8px 4px; font-size: 11.5px; color: var(--dim); text-transform: uppercase; letter-spacing: .05em; font-weight: 600; }
.players button { display: flex; align-items: center; gap: 8px; border: 0; background: transparent; color: var(--text); font: inherit; text-align: left;
  padding: 7px 8px; border-radius: 9px; cursor: pointer; transition: background .15s; }
.players button:hover { background: var(--raise); }
.loadbar { position: fixed; top: 0; left: 0; height: 3px; width: 100%; z-index: 5; transform-origin: left; transform: scaleX(0); opacity: 0;
  background: var(--accent); transition: transform .3s var(--ease), opacity .3s; }
.loadbar.on { opacity: 1; animation: slide 1.1s ease-in-out infinite; }
.icon svg { width: 18px; height: 18px; }
.toast { position: fixed; left: 50%; bottom: 22px; z-index: 6; padding: 9px 16px; border-radius: 12px; background: var(--text); color: var(--bg);
  font-size: 13px; font-weight: 600; box-shadow: var(--shadow); transform: translate(-50%, 12px); opacity: 0; pointer-events: none;
  transition: opacity .2s, transform .2s var(--ease); }
.toast.on { opacity: 1; transform: translate(-50%, 0); }
.empty { position: fixed; inset: 0; display: none; place-items: center; text-align: center; color: var(--dim); padding: 24px; pointer-events: none; z-index: 2; }
.empty.on { display: grid; animation: rise .5s var(--ease) both; }
@keyframes rise { from { opacity: 0; transform: translateY(8px); } to { opacity: 1; transform: none; } }
@keyframes slide { 0% { transform: translateX(-100%) scaleX(.4); } 100% { transform: translateX(100%) scaleX(.4); } }
@media (max-width: 640px) {
  .top-right { top: auto; bottom: 14px; right: 14px; }
  .bottom-left { bottom: 62px; }
  .bottom-right { bottom: 62px; }
  .pill strong { max-width: 38vw; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
}
@media (prefers-reduced-motion: reduce) { * { animation: none !important; transition: none !important; } }
</style>
</head>
<body>
<canvas id="map" tabindex="0"></canvas>
<div class="loadbar" id="load"></div>
<div class="bar top-left"><div class="pill"><strong id="title">World map</strong></div></div>
<div class="bar top-right">
  <div class="seg" id="realms"></div>
  <button class="icon" id="link" title="Copy link"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/></svg></button>
  <button class="icon" id="night" title="Night">&#9790;</button>
  <button class="icon" id="in" title="Zoom in">+</button>
  <button class="icon" id="out" title="Zoom out">&#8722;</button>
</div>
<div class="bar bottom-left"><div class="pill coords" id="coords">X <b>0</b> Z <b>0</b></div></div>
<div class="bar bottom-right"><div class="players" id="people" hidden></div></div>
<div class="toast" id="toast"></div>
<div class="empty" id="empty"><div><div style="font-size:34px">&#9638;</div><p id="emptyText">This dimension hasn't been generated yet.</p></div></div>
<script>
(function () {
  var L = {
    title: "World map", loading: "Loading...", missing: "This dimension hasn't been generated yet.",
    zoomIn: "Zoom in", zoomOut: "Zoom out", night: "Night", players: "Players", none: "No one is online.", copyLink: "Copy a link to this spot", copied: "Copied the link.",
    "realm.overworld": "Overworld", "realm.overworld-cave": "Caves", "realm.nether": "Nether", "realm.end": "End"
  };
  var canvas = document.getElementById("map");
  var ctx = canvas.getContext("2d");
  var TILE = 128, MIN_Z = -3, MAX_Z = 5;
  var state = { realm: "", x: 0, z: 0, scale: 1, night: false, bounds: null };
  var tiles = {}, inflight = 0, queue = [], realms = [], showPeople = false, people = [], dpr = 1, w = 0, h = 0;
  var pointers = {}, last = null, velocity = { x: 0, z: 0 }, glide = 0, dirty = true, hashTimer = 0, loading = 0, aim = null;

  function el(id) { return document.getElementById(id); }
  function text(key, fallback) { return L[key] || fallback || key; }
  function get(url) { return fetch(url, { cache: "no-store" }).then(function (r) { if (!r.ok) throw new Error(r.status); return r.json(); }); }

  function resize() {
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    w = window.innerWidth; h = window.innerHeight;
    canvas.width = Math.round(w * dpr); canvas.height = Math.round(h * dpr);
    dirty = true;
  }

  function levelFor(scale) {
    var z = Math.round(Math.log(1 / scale) / Math.LN2);
    return Math.max(MIN_Z, Math.min(MAX_Z, z));
  }
  function key(z, tx, ty) { return state.realm + "|" + (state.night ? "n" : "d") + "|" + z + "|" + tx + "|" + ty; }

  function wanted(z, pad) {
    var span = TILE * Math.pow(2, z);
    var half = { x: w / 2 / state.scale, z: h / 2 / state.scale };
    var x0 = Math.floor((state.x - half.x) / span) - pad, x1 = Math.floor((state.x + half.x) / span) + pad;
    var z0 = Math.floor((state.z - half.z) / span) - pad, z1 = Math.floor((state.z + half.z) / span) + pad;
    var list = [];
    for (var ty = z0; ty <= z1; ty++) for (var tx = x0; tx <= x1; tx++) list.push([tx, ty]);
    return { list: list, span: span };
  }

  function request(z, tx, ty) {
    var k = key(z, tx, ty), t = tiles[k], now = Date.now();
    var span = TILE * Math.pow(2, z), b = state.bounds;
    if (b && (tx * span >= b[2] || (tx + 1) * span <= b[0] || ty * span >= b[3] || (ty + 1) * span <= b[1])) return;
    if (t && (t.busy || (t.empty && now - t.at < 30000) || (t.img && now - t.at < 180000))) return;
    if (!t) t = tiles[k] = { img: null, at: 0, shown: 0, busy: false, empty: false };
    t.busy = true;
    queue.push({ t: t, z: z, tx: tx, ty: ty, realm: state.realm, night: state.night });
  }

  function pump() {
    while (inflight < 6 && queue.length) {
      var best = 0, bestD = Infinity, span;
      for (var i = 0; i < queue.length; i++) {
        var q = queue[i];
        span = TILE * Math.pow(2, q.z);
        var d = Math.abs((q.tx + .5) * span - state.x) + Math.abs((q.ty + .5) * span - state.z);
        if (q.realm !== state.realm) d += 1e9;
        if (d < bestD) { bestD = d; best = i; }
      }
      var job = queue.splice(best, 1)[0];
      if (job.realm !== state.realm || job.night !== state.night) { job.t.busy = false; continue; }
      load(job);
    }
    el("load").classList.toggle("on", inflight > 0 || queue.length > 0);
  }

  function load(job) {
    inflight++;
    var img = new Image();
    img.decoding = "async";
    var url = "tile/" + encodeURIComponent(job.realm) + "/" + (job.night ? "n" : "d") + "/" + job.z + "/" + job.tx + "/" + job.ty + ".png";
    img.onload = function () {
      inflight--; job.t.busy = false; job.t.img = img; job.t.empty = false; job.t.at = Date.now(); job.t.shown = job.t.shown || performance.now();
      dirty = true; pump();
    };
    img.onerror = function () {
      inflight--; job.t.busy = false; job.t.empty = !job.t.img; job.t.at = Date.now(); pump();
    };
    img.src = url;
  }

  function drawTile(t, z, tx, ty, alpha) {
    var span = TILE * Math.pow(2, z);
    var sx = (tx * span - state.x) * state.scale + w / 2;
    var sy = (ty * span - state.z) * state.scale + h / 2;
    var size = span * state.scale;
    ctx.globalAlpha = alpha;
    ctx.drawImage(t.img, Math.floor(sx), Math.floor(sy), Math.ceil(size) + 1, Math.ceil(size) + 1);
  }

  function fallback(z, tx, ty) {
    for (var up = 1; up <= 4 && z + up <= MAX_Z; up++) {
      var pz = z + up, f = Math.pow(2, up);
      var px = Math.floor(tx / f), py = Math.floor(ty / f);
      var t = tiles[key(pz, px, py)];
      if (t && t.img) {
        var cell = TILE / f;
        var cx = (tx - px * f) * cell, cy = (ty - py * f) * cell;
        var span = TILE * Math.pow(2, z);
        var sx = (tx * span - state.x) * state.scale + w / 2;
        var sy = (ty * span - state.z) * state.scale + h / 2;
        var size = span * state.scale;
        ctx.globalAlpha = 1;
        ctx.drawImage(t.img, cx, cy, cell, cell, Math.floor(sx), Math.floor(sy), Math.ceil(size) + 1, Math.ceil(size) + 1);
        return true;
      }
    }
    return false;
  }

  function draw(now) {
    var animating = false;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    if (state.realm) {
      var z = levelFor(state.scale);
      var set = wanted(z, 0);
      var px = TILE * Math.pow(2, z) * state.scale;
      ctx.imageSmoothingEnabled = px < TILE;
      if (!aim) {
        var around = wanted(z, 1).list;
        for (var j = 0; j < around.length; j++) request(z, around[j][0], around[j][1]);
      }
      for (var i = 0; i < set.list.length; i++) {
        var tx = set.list[i][0], ty = set.list[i][1];
        var t = tiles[key(z, tx, ty)];
        if (t && t.img) {
          var age = (now - t.shown) / 160;
          var a = Math.max(0, Math.min(1, age));
          if (a < 1) { fallback(z, tx, ty); animating = true; }
          drawTile(t, z, tx, ty, a);
        } else {
          fallback(z, tx, ty);
        }
      }
      ctx.globalAlpha = 1;
      drawPeople();
    }
    pump();
    dirty = animating;
  }

  var faces = {};
  function face(name) {
    if (!faces[name]) {
      var img = new Image();
      img.onload = function () { dirty = true; };
      img.src = "head?name=" + encodeURIComponent(name);
      faces[name] = img;
    }
    var got = faces[name];
    return got.complete && got.naturalWidth ? got : null;
  }

  function drawPeople() {
    if (!showPeople) return;
    ctx.font = "600 12px system-ui, sans-serif";
    ctx.textAlign = "center";
    for (var i = 0; i < people.length; i++) {
      var p = people[i];
      if (p.realm !== state.realm.replace("-cave", "")) continue;
      var sx = (p.x - state.x) * state.scale + w / 2, sy = (p.z - state.z) * state.scale + h / 2;
      if (sx < -40 || sy < -40 || sx > w + 40 || sy > h + 40) continue;
      ctx.fillStyle = "rgba(0,0,0,.55)";
      var tw = ctx.measureText(p.name).width + 14;
      ctx.beginPath(); ctx.roundRect(sx - tw / 2, sy - 38, tw, 20, 10); ctx.fill();
      ctx.fillStyle = "#fff"; ctx.fillText(p.name, sx, sy - 24);
      var pic = face(p.name);
      if (pic) {
        ctx.fillStyle = "#fff"; ctx.fillRect(sx - 12, sy - 12, 24, 24);
        var smooth = ctx.imageSmoothingEnabled; ctx.imageSmoothingEnabled = false;
        ctx.drawImage(pic, sx - 10, sy - 10, 20, 20);
        ctx.imageSmoothingEnabled = smooth;
      } else {
        ctx.beginPath(); ctx.arc(sx, sy, 5, 0, 7); ctx.fillStyle = "#fff"; ctx.fill();
        ctx.lineWidth = 2; ctx.strokeStyle = "rgba(0,0,0,.7)"; ctx.stroke();
      }
    }
  }

  function frame(now) {
    if (aim) {
      var from = Math.log(state.scale), to = Math.log(aim.scale);
      if (Math.abs(to - from) < 0.003) { scaleAt(aim.scale, aim.cx, aim.cy); aim = null; }
      else scaleAt(Math.exp(from + (to - from) * 0.28), aim.cx, aim.cy);
      dirty = true;
    }
    if (glide) {
      state.x -= velocity.x / state.scale; state.z -= velocity.z / state.scale;
      velocity.x *= .92; velocity.z *= .92;
      if (Math.abs(velocity.x) + Math.abs(velocity.z) < .05) glide = 0;
      dirty = true; moved();
    }
    if (dirty) draw(now);
    requestAnimationFrame(frame);
  }

  function moved() {
    var b = state.bounds;
    if (b) {
      state.x = Math.max(b[0] - 1024, Math.min(b[2] + 1024, state.x));
      state.z = Math.max(b[1] - 1024, Math.min(b[3] + 1024, state.z));
    }
    clearTimeout(hashTimer);
    hashTimer = setTimeout(saveHash, 400);
  }

  function saveHash() {
    var v = [state.realm, Math.round(state.x), Math.round(state.z), state.scale.toFixed(3), state.night ? 1 : 0].join(",");
    try { history.replaceState(null, "", "#" + v); } catch (e) { }
  }

  function scaleAt(next, cx, cy) {
    var bx = (cx - w / 2) / state.scale + state.x, bz = (cy - h / 2) / state.scale + state.z;
    state.scale = next;
    state.x = bx - (cx - w / 2) / next; state.z = bz - (cy - h / 2) / next;
    moved();
  }

  function zoomAt(factor, cx, cy, instant) {
    var base = aim ? aim.scale : state.scale;
    var next = Math.max(0.0625, Math.min(16, base * factor));
    if (instant) { aim = null; if (next !== state.scale) scaleAt(next, cx, cy); dirty = true; return; }
    if (next === base) return;
    aim = { scale: next, cx: cx, cy: cy };
    dirty = true;
  }

  function showCoords(cx, cy) {
    var bx = Math.floor((cx - w / 2) / state.scale + state.x), bz = Math.floor((cy - h / 2) / state.scale + state.z);
    el("coords").innerHTML = "X <b>" + bx + "</b> &nbsp;Z <b>" + bz + "</b>";
  }

  canvas.addEventListener("pointerdown", function (e) {
    canvas.setPointerCapture(e.pointerId); pointers[e.pointerId] = { x: e.clientX, y: e.clientY };
    canvas.classList.add("drag"); glide = 0; last = null;
  });
  canvas.addEventListener("pointermove", function (e) {
    showCoords(e.clientX, e.clientY);
    var p = pointers[e.pointerId]; if (!p) return;
    var ids = Object.keys(pointers);
    if (ids.length === 2) {
      var a = pointers[ids[0]], b = pointers[ids[1]];
      var before = Math.hypot(a.x - b.x, a.y - b.y);
      p.x = e.clientX; p.y = e.clientY;
      var after = Math.hypot(a.x - b.x, a.y - b.y);
      if (before > 0) zoomAt(after / before, (a.x + b.x) / 2, (a.y + b.y) / 2, true);
      return;
    }
    var dx = e.clientX - p.x, dy = e.clientY - p.y;
    p.x = e.clientX; p.y = e.clientY;
    state.x -= dx / state.scale; state.z -= dy / state.scale;
    velocity.x = dx; velocity.z = dy;
    dirty = true; moved();
  });
  function release(e) {
    delete pointers[e.pointerId];
    if (!Object.keys(pointers).length) {
      canvas.classList.remove("drag");
      if (Math.abs(velocity.x) + Math.abs(velocity.z) > 3) glide = 1;
    }
  }
  canvas.addEventListener("pointerup", release);
  canvas.addEventListener("pointercancel", release);
  canvas.addEventListener("wheel", function (e) {
    e.preventDefault();
    zoomAt(Math.pow(1.0022, -e.deltaY * (e.deltaMode === 1 ? 16 : 1)), e.clientX, e.clientY);
  }, { passive: false });
  canvas.addEventListener("dblclick", function (e) { zoomAt(2, e.clientX, e.clientY); });
  canvas.addEventListener("keydown", function (e) {
    var step = 80 / state.scale;
    if (e.key === "ArrowLeft") state.x -= step; else if (e.key === "ArrowRight") state.x += step;
    else if (e.key === "ArrowUp") state.z -= step; else if (e.key === "ArrowDown") state.z += step;
    else if (e.key === "+" || e.key === "=") zoomAt(1.5, w / 2, h / 2); else if (e.key === "-") zoomAt(1 / 1.5, w / 2, h / 2);
    else return;
    e.preventDefault(); dirty = true; moved();
  });
  window.addEventListener("resize", resize);

  el("in").onclick = function () { zoomAt(1.6, w / 2, h / 2); };
  el("out").onclick = function () { zoomAt(1 / 1.6, w / 2, h / 2); };
  var toastTimer = 0;
  function toast(message) {
    var box = el("toast"); box.textContent = message; box.classList.add("on");
    clearTimeout(toastTimer); toastTimer = setTimeout(function () { box.classList.remove("on"); }, 1600);
  }
  function copyText(value) {
    if (window.isSecureContext && navigator.clipboard) return navigator.clipboard.writeText(value);
    var area = document.createElement("textarea");
    area.value = value; area.setAttribute("readonly", ""); area.style.position = "fixed"; area.style.opacity = "0";
    document.body.appendChild(area); area.select();
    var ok = false;
    try { ok = document.execCommand("copy"); } catch (e) { }
    area.remove();
    return ok ? Promise.resolve() : Promise.reject(new Error("copy"));
  }
  el("link").onclick = function () {
    clearTimeout(hashTimer); saveHash();
    copyText(location.href).then(function () { toast(text("copied")); }, function () { window.prompt("", location.href); });
  };
  el("night").onclick = function () {
    state.night = !state.night; el("night").classList.toggle("on", state.night); queue = []; dirty = true; moved();
  };

  function pretty(r) {
    var name = r.indexOf(".") > 0 ? r.slice(r.indexOf(".") + 1) : r;
    return name.split(/[-_.]+/).map(function (word) { return word.charAt(0).toUpperCase() + word.slice(1); }).join(" ");
  }

  function buildRealms() {
    var box = el("realms"); box.innerHTML = "";
    realms.forEach(function (r) {
      var b = document.createElement("button");
      b.textContent = text("realm." + r, pretty(r));
      b.className = r === state.realm ? "on" : "";
      b.onclick = function () { switchRealm(r, true); };
      box.appendChild(b);
    });
    box.style.display = realms.length > 1 ? "flex" : "none";
  }

  function switchRealm(r, fit) {
    state.realm = r; queue = []; buildRealms();
    get("bounds?realm=" + encodeURIComponent(r)).then(function (b) {
      state.bounds = b;
      if (fit) {
        var inside = b[0] + 64 < 0 && b[2] - 64 > 0 && b[1] + 64 < 0 && b[3] - 64 > 0;
        state.x = inside ? 0 : (b[0] + b[2]) / 2; state.z = inside ? 0 : (b[1] + b[3]) / 2;
        state.scale = Math.max(0.0625, Math.min(2, Math.min(w / (b[2] - b[0] + 256), h / (b[3] - b[1] + 256))));
      }
      el("empty").classList.remove("on"); dirty = true; moved();
    }, function () {
      state.bounds = null; el("emptyText").textContent = text("missing"); el("empty").classList.add("on"); dirty = true;
    });
  }

  function pollPeople() {
    if (!showPeople) return;
    get("players").then(function (list) {
      people = list; dirty = true;
      var box = el("people"); box.hidden = false; box.innerHTML = "";
      var title = document.createElement("h4"); title.textContent = text("players") + " · " + list.length; box.appendChild(title);
      if (!list.length) {
        var none = document.createElement("p"); none.textContent = text("none"); box.appendChild(none);
        return;
      }
      list.forEach(function (p) {
        var b = document.createElement("button");
        var letter = document.createElement("span"); letter.className = "face"; letter.textContent = p.name.slice(0, 1).toUpperCase();
        var pic = document.createElement("img"); pic.alt = ""; pic.src = "head?name=" + encodeURIComponent(p.name);
        pic.onerror = function () { pic.replaceWith(letter); };
        b.appendChild(pic); b.appendChild(document.createTextNode(p.name));
        b.onclick = function () {
          var target = realms.indexOf(p.realm) >= 0 ? p.realm : state.realm;
          if (target !== state.realm) switchRealm(target, false);
          state.x = p.x; state.z = p.z; aim = null; zoomAt(Math.max(state.scale, 1) / state.scale, w / 2, h / 2); dirty = true; moved();
        };
        box.appendChild(b);
      });
    }, function () { });
  }

  function applyHash() {
    var parts = decodeURIComponent(location.hash.replace("#", "")).split(",");
    if (parts.length >= 5 && realms.indexOf(parts[0]) >= 0) {
      var x = parseFloat(parts[1]), z = parseFloat(parts[2]), s = parseFloat(parts[3]);
      if (isFinite(x) && isFinite(z) && isFinite(s)) return { realm: parts[0], x: x, z: z, scale: s, night: parts[4] === "1" };
    }
    return null;
  }

  function init() {
    resize();
    get("info").then(function (info) {
      for (var k in info.labels) L[k] = info.labels[k];
      var name = info.name ? L.title.indexOf("{0}") >= 0 ? L.title.replace("{0}", info.name) : info.name : L.title;
      el("title").textContent = name; document.title = name;
      el("in").title = text("zoomIn"); el("out").title = text("zoomOut"); el("night").title = text("night"); el("link").title = text("copyLink");
      realms = info.realms; showPeople = !!info.players;
      var saved = applyHash();
      if (!realms.length) { el("emptyText").textContent = text("missing"); el("empty").classList.add("on"); return; }
      var first = saved ? saved.realm : (realms.indexOf("overworld") >= 0 ? "overworld" : realms[0]);
      if (saved) { state.night = saved.night; el("night").classList.toggle("on", saved.night); }
      state.realm = first; buildRealms();
      get("bounds?realm=" + encodeURIComponent(first)).then(function (b) {
        state.bounds = b;
        if (saved) { state.x = saved.x; state.z = saved.z; state.scale = saved.scale; }
        else { switchRealm(first, true); return; }
        dirty = true;
      }, function () { switchRealm(first, true); });
      pollPeople(); setInterval(pollPeople, 3000);
    }, function () { el("emptyText").textContent = text("missing"); el("empty").classList.add("on"); });
    requestAnimationFrame(frame);
  }
  init();
})();
</script>
</body>
</html>
""";
}
