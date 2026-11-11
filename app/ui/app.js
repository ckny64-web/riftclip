"use strict";
// Riftclip library window. Plain JavaScript, no build step.

const tauri = window.__TAURI__;
const invoke = (cmd, args) => tauri.core.invoke(cmd, args);
const fileUrl = (path) => (path ? tauri.core.convertFileSrc(path) : "");
const $ = (id) => document.getElementById(id);

// ---------- small helpers ----------

/** Build an element: h("div", {class: "x", onclick: f}, child, "text", ...) */
function h(tag, props, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (v === false || v === null || v === undefined) continue;
    if (k === "class") el.className = v;
    else if (k === "style") el.style.cssText = v;
    else if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
    else if (k in el && k !== "list") el[k] = v;
    else el.setAttribute(k, v);
  }
  for (const c of children.flat()) {
    if (c === null || c === undefined || c === false) continue;
    el.append(c.nodeType ? c : document.createTextNode(String(c)));
  }
  return el;
}

function clock(secs) {
  secs = Math.max(0, Math.floor(secs || 0));
  const hrs = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = String(secs % 60).padStart(2, "0");
  return hrs ? `${hrs}:${String(m).padStart(2, "0")}:${s}` : `${String(m).padStart(2, "0")}:${s}`;
}
const dateText = (unix) => new Date(unix * 1000).toLocaleString([], { dateStyle: "medium", timeStyle: "short" });
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

function toast(message, isError) {
  const el = h("div", { class: "toast" + (isError ? " error" : "") }, String(message));
  $("toasts").append(el);
  setTimeout(() => el.remove(), isError ? 8000 : 3500);
}
const fail = (e) => toast(e && e.message ? e.message : e, true);

function closeModal() {
  $("modal").hidden = true;
  $("modal-box").replaceChildren();
}
function openModal(wide, ...content) {
  const box = $("modal-box");
  box.className = "modal-box" + (wide ? " wide" : "");
  box.replaceChildren(...content);
  $("modal").hidden = false;
}
$("modal").addEventListener("mousedown", (e) => { if (e.target === $("modal")) closeModal(); });

/** Ask before something destructive. Resolves true if confirmed. */
function confirmBox(title, text, actionLabel) {
  return new Promise((resolve) => {
    const done = (v) => { closeModal(); resolve(v); };
    openModal(false,
      h("h3", {}, title), h("div", { class: "sub" }, text),
      h("div", { class: "btns" },
        h("button", { onclick: () => done(false) }, "Cancel"),
        h("button", { class: "danger", onclick: () => done(true) }, actionLabel)));
  });
}

/** Ask for a line of text. Resolves the text, or null if cancelled. */
function promptBox(title, initial, actionLabel) {
  return new Promise((resolve) => {
    const input = h("input", { type: "text", value: initial || "", maxLength: 80 });
    const done = (v) => { closeModal(); resolve(v); };
    const ok = () => done(input.value.trim() || null);
    input.addEventListener("keydown", (e) => { if (e.key === "Enter") ok(); if (e.key === "Escape") done(null); });
    openModal(false, h("h3", {}, title), input,
      h("div", { class: "btns" },
        h("button", { onclick: () => done(null) }, "Cancel"),
        h("button", { class: "primary", onclick: ok }, actionLabel)));
    input.focus();
    input.select();
  });
}

// Hover tooltip for timeline markers.
function showTip(text, x, y) {
  const tip = $("tip");
  tip.textContent = text;
  tip.hidden = false;
  const w = tip.offsetWidth;
  tip.style.left = clamp(x - w / 2, 6, window.innerWidth - w - 6) + "px";
  tip.style.top = y - 34 + "px";
}
const hideTip = () => { $("tip").hidden = true; };

// ---------- event presentation ----------

/** CSS class for an event's colour. */
function eventKind(e) {
  if (e.type === "manual") return "k-mark";
  const objective = ["dragon", "baron", "herald", "voidgrubs", "atakhan", "turret", "inhibitor"].includes(e.type);
  if (objective) return ["killer", "assist", "team"].includes(e.involvement) ? "k-obj" : "k-enemy";
  if (e.involvement === "killer") return "k-kill";
  if (e.involvement === "victim") return "k-death";
  if (e.involvement === "assist") return "k-assist";
  if (e.type === "ace") return e.involvement === "team" ? "k-obj" : "k-enemy";
  return "k-other";
}
/** Is this one of my own moments (as opposed to something others did)? */
const isMine = (e) => e.type === "manual" || ["killer", "victim", "assist"].includes(e.involvement);
const isObjective = (e) => ["dragon", "baron", "herald", "voidgrubs", "atakhan", "turret", "inhibitor", "ace"].includes(e.type);
const hiddenTypes = ["game_start", "game_end"];

function visibleEvents(events, filter) {
  return events.filter((e) => {
    if (hiddenTypes.includes(e.type)) return false;
    if (filter === "all") return true;
    if (filter === "mine") return isMine(e);
    return isMine(e) || isObjective(e); // "key": my plays and objectives
  });
}

function rankText(p) {
  if (!p.rank_tier) return "";
  const tier = p.rank_tier[0] + p.rank_tier.slice(1).toLowerCase();
  return tier + (p.rank_division ? " " + p.rank_division : "") + " · " + p.rank_lp + " LP";
}

// ---------- state ----------

const state = {
  tab: "matches",
  matches: [],
  selectedId: null,
  details: null,       // match_details of the selected match
  eventFilter: "key",
  playingClip: null,   // a clip row when the player shows a clip instead of the match
  sel: null,           // {start, end} selection for a new clip, in seconds
  clipFilter: "all",
  settings: null,
  status: { text: "", match_id: null, recording: false, since: null },
  padBefore: 12,
  padAfter: 6,
};
let video = null;      // the match player's <video>
let tl = null;         // timeline element refs

// ---------- tabs ----------

function showTab(tab) {
  state.tab = tab;
  for (const b of $("tabs").children) b.classList.toggle("active", b.dataset.tab === tab);
  for (const t of ["matches", "clips", "settings"]) $("view-" + t).hidden = t !== tab;
  if (tab !== "matches" && video) video.pause();
  if (tab === "matches") loadMatches().catch(fail);
  if (tab === "clips") loadClips().catch(fail);
  if (tab === "settings") loadSettings().catch(fail);
}
for (const b of $("tabs").children) b.addEventListener("click", () => showTab(b.dataset.tab));

// ---------- status line ----------

function renderStatus() {
  const s = state.status;
  const el = $("status");
  el.classList.toggle("recording", !!s.recording);
  el.classList.toggle("ingame", !!s.match_id && !s.recording);
  let text = s.text || "";
  if (s.since) text += "  ·  " + clock(Date.now() / 1000 - s.since);
  $("status-text").textContent = text;
}
async function pollStatus() {
  try {
    const before = state.status;
    state.status = await invoke("get_status");
    renderStatus();
    // A match started or finished, or its files were just saved: refresh the list.
    if (before.match_id !== state.status.match_id || before.text !== state.status.text) {
      if (state.tab === "matches") await loadMatches();
    }
  } catch (_) { /* the app is shutting down */ }
}
setInterval(() => { if (!document.hidden) pollStatus(); }, 2000);
setInterval(renderStatus, 1000);

// ---------- match list ----------

async function loadMatches() {
  state.matches = await invoke("list_matches");
  renderMatchList();
  if (state.selectedId === null && state.matches.length) {
    await selectMatch(state.matches[0].id);
  } else if (state.selectedId !== null && !state.matches.some((m) => m.id === state.selectedId)) {
    state.selectedId = null;
    state.details = null;
    renderMatchView();
  } else if (state.selectedId !== null) {
    await refreshDetails();
  } else {
    renderMatchView();
  }
}

function renderMatchList() {
  const list = $("match-list");
  if (!state.matches.length) {
    list.replaceChildren(h("div", { class: "empty" }, "No matches yet. Play a game with Riftclip running and it will show up here."));
    return;
  }
  list.replaceChildren(...state.matches.map((m) => {
    const live = state.status.match_id === m.id;
    const result = m.result === "win" ? "Victory" : m.result === "loss" ? "Defeat" : "";
    const thumb = h("div", { class: "thumb" + (m.thumbnail_path ? "" : " none") },
      live && h("span", { class: "live" }, "LIVE"),
      m.duration > 0 && h("span", { class: "badge" }, clock(m.duration)));
    if (m.thumbnail_path) thumb.style.backgroundImage = `url("${fileUrl(m.thumbnail_path)}")`;
    return h("div", { class: "match-card" + (m.id === state.selectedId ? " selected" : ""), onclick: () => selectMatch(m.id).catch(fail) },
      thumb,
      h("div", { class: "info" },
        h("div", { class: "title" },
          h("span", {}, m.champion || "Unknown"),
          h("span", { class: "sub" }, `${m.kills} / ${m.deaths} / ${m.assists}`),
          h("span", { class: "res " + (m.result === "win" ? "win" : m.result === "loss" ? "loss" : "sub") }, result)),
        h("div", { class: "sub" }, dateText(m.started_at)),
        h("div", { class: "sub" }, [m.queue || m.game_mode, m.clip_count ? `${m.clip_count} clip${m.clip_count === 1 ? "" : "s"}` : ""].filter(Boolean).join("  ·  "))));
  }));
}

async function selectMatch(id) {
  state.selectedId = id;
  state.playingClip = null;
  state.sel = null;
  state.details = await invoke("match_details", { id });
  renderMatchList();
  renderMatchView();
}

/** Re-read the selected match without rebuilding the player (keeps playback going). */
async function refreshDetails() {
  if (state.selectedId === null) return;
  const before = state.details;
  state.details = await invoke("match_details", { id: state.selectedId });
  const sameVideo = before && before.match.recording_path === state.details.match.recording_path;
  if (sameVideo && video) {
    renderLower();
    renderTimeline();
  } else {
    renderMatchView();
  }
}

// ---------- match view ----------

function duration() {
  if (video && isFinite(video.duration) && video.duration > 0 && !state.playingClip) return video.duration;
  return state.details ? state.details.match.duration : 0;
}

function seek(t, play) {
  if (!video) return;
  if (state.playingClip) backToMatch(t, play);
  else {
    video.currentTime = clamp(t, 0, Math.max(0, duration() - 0.05));
    if (play) video.play().catch(() => {});
  }
}

function renderMatchView() {
  const view = $("match-view");
  const d = state.details;
  video = null;
  tl = null;
  if (!d) {
    view.replaceChildren(h("div", { class: "empty" }, state.matches.length ? "Select a match." : ""));
    return;
  }
  const m = d.match;
  const me = d.players.find((p) => p.is_me);
  const live = state.status.match_id === m.id;
  const result = m.result === "win" ? "Victory" : m.result === "loss" ? "Defeat" : "";

  const head = h("div", { class: "mv-head" },
    h("h2", {}, m.champion || "Match"),
    result && h("span", { class: m.result === "win" ? "win" : "loss" }, result),
    me && h("span", {}, `${me.kills} / ${me.deaths} / ${me.assists}`),
    h("span", { class: "sub" }, [m.queue || m.game_mode, dateText(m.started_at), clock(m.duration)].filter(Boolean).join("  ·  ")),
    h("div", { class: "actions" },
      m.recording_path && h("button", { class: "ghost", onclick: () => invoke("show_in_folder", { path: m.recording_path }).catch(fail) }, "Show file"),
      m.recording_path && !live && h("button", { class: "ghost danger", onclick: deleteRecording }, "Delete recording"),
      !live && h("button", { class: "ghost danger", onclick: deleteMatch }, "Delete match")));

  const player = h("div", { class: "player", id: "player" });
  const parts = [head, h("div", { id: "clip-banner" }), player];

  if (live) {
    player.append(h("div", { class: "notice" }, "This match is being played right now. The recording will be available when it ends."));
  } else if (m.recording_path) {
    video = h("video", { src: fileUrl(m.recording_path), preload: "metadata", controls: false });
    video.addEventListener("click", togglePlay);
    video.addEventListener("timeupdate", updatePlayhead);
    video.addEventListener("loadedmetadata", () => { renderTimeline(); updatePlayhead(); });
    video.addEventListener("play", updatePlayButton);
    video.addEventListener("pause", updatePlayButton);
    video.addEventListener("error", () => {
      if (!player.querySelector(".notice")) {
        player.append(h("div", { class: "notice" }, "This video could not be loaded. The file may have been moved or deleted."));
      }
    });
    player.append(video);
    parts.push(h("div", { class: "timeline", id: "timeline" }), h("div", { class: "controls", id: "controls" }));
  } else {
    player.append(h("div", { class: "notice" }, "The full recording of this match is no longer kept. Its clips and timeline are still here."));
  }
  parts.push(h("div", { class: "lower", id: "lower" }));
  view.replaceChildren(...parts);
  if (video) {
    renderControls();
    renderTimeline();
  }
  renderLower();
}

function togglePlay() {
  if (!video) return;
  if (video.paused) video.play().catch(() => {}); else video.pause();
}
function updatePlayButton() {
  const b = $("play-btn");
  if (b && video) b.textContent = video.paused ? "Play" : "Pause";
}

function renderControls() {
  const box = $("controls");
  if (!box || !video) return;
  const titleInput = h("input", { type: "text", id: "clip-title", placeholder: "Clip name", maxLength: 80 });
  titleInput.addEventListener("keydown", (e) => { e.stopPropagation(); if (e.key === "Enter") saveClip(); });
  const volume = h("input", { type: "range", min: 0, max: 1, step: 0.05, value: video.volume, title: "Volume" });
  volume.addEventListener("input", () => { video.volume = Number(volume.value); video.muted = false; });
  const speed = h("select", { title: "Playback speed" }, ...[0.25, 0.5, 1, 1.5, 2].map((s) => h("option", { value: s, selected: s === 1 }, s + "x")));
  speed.addEventListener("change", () => { video.playbackRate = Number(speed.value); });
  box.replaceChildren(
    h("button", { id: "play-btn", onclick: togglePlay }, "Play"),
    h("span", { class: "time", id: "time-label" }, "00:00 / 00:00"),
    volume, speed,
    h("span", { class: "gap" }),
    h("button", { onclick: () => setSel("start"), title: "Start the clip at the current time (I)" }, "Set start"),
    h("button", { onclick: () => setSel("end"), title: "End the clip at the current time (O)" }, "Set end"),
    h("span", { class: "sel", id: "sel-label" }),
    titleInput,
    h("button", { class: "primary", id: "save-clip", onclick: saveClip, disabled: true }, "Save clip"),
    h("button", { class: "ghost", id: "clear-sel", onclick: () => { state.sel = null; renderSelection(); }, hidden: true }, "Clear"));
  renderSelection();
}

// ---------- timeline ----------

function renderTimeline() {
  const box = $("timeline");
  if (!box || !state.details) return;
  const total = duration();
  const track = h("div", { class: "tl-track" });
  const progress = h("div", { class: "tl-progress" });
  const sel = h("div", { class: "tl-sel", hidden: true });
  track.append(progress, sel);

  const pct = (t) => (total > 0 ? clamp(t / total, 0, 1) * 100 : 0) + "%";
  // Saved clips as faint bands.
  if (!state.playingClip) {
    for (const c of state.details.clips) {
      const band = h("div", { class: "tl-clip" });
      band.style.left = pct(c.start);
      band.style.width = (total > 0 ? clamp((c.end - c.start) / total, 0, 1) * 100 : 0) + "%";
      track.append(band);
    }
  }

  const markers = state.playingClip ? [] : visibleEvents(state.details.events, state.eventFilter).map((e) => {
    const mk = h("div", { class: "tl-marker " + eventKind(e) + (e.type === "multikill" || e.type === "baron" ? " big" : "") });
    mk.style.left = pct(e.rec_time);
    mk.addEventListener("mouseenter", () => { const r = mk.getBoundingClientRect(); showTip(`${clock(e.rec_time)}  ${e.detail}`, r.left + r.width / 2, r.top); });
    mk.addEventListener("mouseleave", hideTip);
    mk.addEventListener("click", (ev) => { ev.stopPropagation(); seek(e.rec_time - 5, true); });
    return mk;
  });

  const playhead = h("div", { class: "tl-playhead" });
  const hStart = h("div", { class: "tl-handle", hidden: true, title: "Drag to move the clip start" });
  const hEnd = h("div", { class: "tl-handle", hidden: true, title: "Drag to move the clip end" });
  box.replaceChildren(track, ...markers, playhead, hStart, hEnd);
  tl = { box, track, progress, sel, playhead, hStart, hEnd };

  // Click or drag on the track to scrub.
  const timeAt = (clientX) => {
    const r = track.getBoundingClientRect();
    return clamp((clientX - r.left) / r.width, 0, 1) * duration();
  };
  track.addEventListener("pointerdown", (e) => {
    track.setPointerCapture(e.pointerId);
    const move = (ev) => { if (video) video.currentTime = timeAt(ev.clientX); };
    move(e);
    const up = () => { track.removeEventListener("pointermove", move); track.removeEventListener("pointerup", up); };
    track.addEventListener("pointermove", move);
    track.addEventListener("pointerup", up);
  });
  // Drag the selection handles.
  for (const [handle, edge] of [[hStart, "start"], [hEnd, "end"]]) {
    handle.addEventListener("pointerdown", (e) => {
      e.stopPropagation();
      handle.setPointerCapture(e.pointerId);
      const move = (ev) => {
        if (!state.sel) return;
        const t = timeAt(ev.clientX);
        if (edge === "start") state.sel.start = clamp(t, 0, state.sel.end - 1);
        else state.sel.end = clamp(t, state.sel.start + 1, duration());
        if (video) video.currentTime = edge === "start" ? state.sel.start : state.sel.end;
        renderSelection();
      };
      const up = () => { handle.removeEventListener("pointermove", move); handle.removeEventListener("pointerup", up); };
      handle.addEventListener("pointermove", move);
      handle.addEventListener("pointerup", up);
    });
  }
  renderSelection();
  updatePlayhead();
}

function updatePlayhead() {
  if (!video) return;
  const total = isFinite(video.duration) && video.duration > 0 ? video.duration : duration();
  const label = $("time-label");
  if (label) label.textContent = `${clock(video.currentTime)} / ${clock(total)}`;
  if (!tl) return;
  const p = (total > 0 ? clamp(video.currentTime / total, 0, 1) * 100 : 0) + "%";
  tl.playhead.style.left = p;
  tl.progress.style.width = p;
}

function setSel(edge) {
  if (!video || state.playingClip) return;
  const t = video.currentTime;
  const total = duration();
  if (!state.sel) {
    state.sel = edge === "start" ? { start: t, end: Math.min(total, t + state.padBefore + state.padAfter) } : { start: Math.max(0, t - state.padBefore - state.padAfter), end: t };
  } else if (edge === "start") {
    state.sel.start = t;
    if (state.sel.end < t + 1) state.sel.end = Math.min(total, t + 10);
  } else {
    state.sel.end = t;
    if (state.sel.start > t - 1) state.sel.start = Math.max(0, t - 10);
  }
  renderSelection();
}

function renderSelection() {
  const s = state.sel;
  const total = duration();
  const valid = !!s && !state.playingClip && s.end - s.start >= 1;
  const save = $("save-clip");
  if (save) save.disabled = !valid;
  const clear = $("clear-sel");
  if (clear) clear.hidden = !s;
  const label = $("sel-label");
  if (label) label.textContent = s ? `${clock(s.start)} – ${clock(s.end)}  (${Math.round(s.end - s.start)} s)` : "";
  if (!tl) return;
  const show = !!s && !state.playingClip && total > 0;
  tl.sel.hidden = tl.hStart.hidden = tl.hEnd.hidden = !show;
  if (show) {
    const a = clamp(s.start / total, 0, 1) * 100, b = clamp(s.end / total, 0, 1) * 100;
    tl.sel.style.left = a + "%";
    tl.sel.style.width = b - a + "%";
    tl.hStart.style.left = a + "%";
    tl.hEnd.style.left = b + "%";
  }
}

/** Select the stretch around an event, ready to save as a clip. */
function selectAround(e) {
  if (!video) return;
  if (state.playingClip) backToMatch();
  const total = duration();
  state.sel = { start: Math.max(0, e.rec_time - state.padBefore), end: Math.min(total || Infinity, e.rec_time + state.padAfter) };
  const title = $("clip-title");
  if (title) title.value = e.detail;
  renderSelection();
  seek(state.sel.start, true);
}

async function saveClip() {
  const s = state.sel;
  if (!s || !state.details) return;
  const save = $("save-clip");
  const title = ($("clip-title").value || "").trim() || "Clip";
  save.disabled = true;
  save.textContent = "Saving …";
  try {
    await invoke("create_clip", { matchId: state.details.match.id, start: s.start, end: s.end, title });
    state.sel = null;
    $("clip-title").value = "";
    toast(`Saved clip "${title}"`);
    await refreshDetails();
    state.matches = await invoke("list_matches");
    renderMatchList();
  } catch (e) {
    fail(e);
  } finally {
    save.textContent = "Save clip";
    renderSelection();
  }
}

// ---------- playing a clip inside the match view ----------

function playClip(clip) {
  if (!video) { openClipPlayer(clip); return; }
  state.playingClip = clip;
  video.src = fileUrl(clip.path);
  video.play().catch(() => {});
  $("clip-banner").replaceChildren(h("div", { class: "clip-banner" },
    h("span", {}, "Playing clip: ", h("b", {}, clip.title || "Clip")),
    h("span", { class: "spacer" }),
    h("button", { onclick: () => backToMatch(clip.start, false) }, "Back to full match")));
  renderTimeline();
  renderSelection();
}

function backToMatch(t, play) {
  if (!video || !state.details) return;
  state.playingClip = null;
  $("clip-banner").replaceChildren();
  video.src = fileUrl(state.details.match.recording_path);
  const go = () => {
    video.removeEventListener("loadedmetadata", go);
    if (typeof t === "number") video.currentTime = clamp(t, 0, Math.max(0, video.duration - 0.05));
    if (play) video.play().catch(() => {});
  };
  video.addEventListener("loadedmetadata", go);
}

// ---------- events, clips and players under the player ----------

function renderLower() {
  const box = $("lower");
  const d = state.details;
  if (!box || !d) return;
  const canSeek = !!video;

  // Events
  const filter = h("div", { class: "seg" }, ...[["key", "Key moments"], ["mine", "Mine"], ["all", "Everything"]].map(([id, label]) =>
    h("button", { class: state.eventFilter === id ? "active" : "", onclick: () => { state.eventFilter = id; renderLower(); renderTimeline(); } }, label)));
  const events = visibleEvents(d.events, state.eventFilter);
  const eventRows = events.length ? events.map((e) =>
    h("div", { class: "row" + (canSeek ? " click" : ""), onclick: canSeek ? () => seek(e.rec_time - 5, true) : null },
      h("span", { class: "t" }, clock(e.rec_time)),
      h("span", { class: "pip " + eventKind(e) }),
      h("span", { class: "grow" }, e.detail),
      canSeek && h("span", { class: "acts" }, h("button", { class: "ghost", title: "Select this moment as a clip", onclick: (ev) => { ev.stopPropagation(); selectAround(e); } }, "Clip"))))
    : [h("div", { class: "empty" }, "Nothing to show for this filter.")];
  const eventsCard = h("div", { class: "card" },
    h("header", {}, h("h3", {}, "Timeline"), h("div", { class: "right" }, filter)),
    h("div", { class: "body" }, ...eventRows));

  // Clips
  const clipRows = d.clips.length ? d.clips.map((c) =>
    h("div", { class: "row click", onclick: () => playClip(c) },
      h("span", { class: "t" }, clock(c.start)),
      h("span", { class: "grow" }, c.title || "Clip"),
      h("span", { class: "sub" }, Math.round(c.end - c.start) + " s"),
      c.auto && h("span", { class: "tag" }, "auto"),
      clipActions(c, refreshAfterClipChange)))
    : [h("div", { class: "empty" }, video ? "No clips yet. Use Set start and Set end, or the Clip button on a moment." : "No clips for this match.")];
  const clipsCard = h("div", { class: "card" }, h("header", {}, h("h3", {}, `Clips (${d.clips.length})`)), h("div", { class: "body" }, ...clipRows));

  // Players
  let playersCard = null;
  if (d.players.length) {
    const myTeam = (d.players.find((p) => p.is_me) || d.players[0]).team;
    const ordered = [...d.players.filter((p) => p.team === myTeam), ...d.players.filter((p) => p.team !== myTeam)];
    const firstEnemy = ordered.findIndex((p) => p.team !== myTeam);
    const rows = ordered.map((p, i) => h("tr", { class: (i === firstEnemy && i > 0 ? "sep" : "") },
      h("td", { class: p.is_me ? "me" : "" }, p.champion || "?"),
      h("td", { class: "sub" }, p.name),
      h("td", {}, rankText(p) || "—"),
      h("td", { class: "num" }, `${p.kills} / ${p.deaths} / ${p.assists}`),
      h("td", { class: "num" }, p.cs + " CS")));
    playersCard = h("div", { class: "card" }, h("header", {}, h("h3", {}, "Players")), h("div", { class: "body" }, h("table", { class: "players" }, ...rows)));
  }

  box.replaceChildren(eventsCard, h("div", { style: "display:flex;flex-direction:column;gap:14px;min-width:0" }, clipsCard, playersCard));
}

function clipActions(c, after) {
  const stop = (fn) => (ev) => { ev.stopPropagation(); fn(); };
  return h("span", { class: "acts" },
    h("button", { class: "ghost", title: "Rename", onclick: stop(async () => {
      const name = await promptBox("Rename clip", c.title, "Rename");
      if (name) { try { await invoke("rename_clip", { id: c.id, title: name }); await after(); } catch (e) { fail(e); } }
    }) }, "Rename"),
    h("button", { class: "ghost", title: "Show the file in File Explorer", onclick: stop(() => invoke("show_in_folder", { path: c.path }).catch(fail)) }, "File"),
    h("button", { class: "ghost danger", title: "Delete this clip", onclick: stop(async () => {
      if (await confirmBox("Delete this clip?", `"${c.title || "Clip"}" and its file will be deleted. This cannot be undone.`, "Delete clip")) {
        try {
          if (state.playingClip && state.playingClip.id === c.id) backToMatch();
          await invoke("delete_clip", { id: c.id });
          await after();
        } catch (e) { fail(e); }
      }
    }) }, "Delete"));
}

async function refreshAfterClipChange() {
  await refreshDetails();
  state.matches = await invoke("list_matches");
  renderMatchList();
}

async function deleteRecording() {
  const m = state.details.match;
  if (!(await confirmBox("Delete the full recording?", "The match, its timeline and its clips stay. You will no longer be able to cut new clips from it.", "Delete recording"))) return;
  try {
    if (video) { video.pause(); video.removeAttribute("src"); video.load(); }
    await invoke("delete_recording", { matchId: m.id });
    await selectMatch(m.id);
    state.matches = await invoke("list_matches");
    renderMatchList();
  } catch (e) { fail(e); }
}

async function deleteMatch() {
  const m = state.details.match;
  if (!(await confirmBox("Delete this match?", "Its recording and timeline will be deleted. Clips you already have are kept in the Clips tab.", "Delete match"))) return;
  try {
    if (video) { video.pause(); video.removeAttribute("src"); video.load(); }
    await invoke("delete_match", { matchId: m.id });
    state.selectedId = null;
    state.details = null;
    await loadMatches();
  } catch (e) { fail(e); }
}

// ---------- keyboard ----------

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !$("modal").hidden) { closeModal(); return; }
  if (state.tab !== "matches" || !video || !$("modal").hidden) return;
  if (["INPUT", "SELECT", "TEXTAREA"].includes(document.activeElement.tagName)) return;
  const k = e.key.toLowerCase();
  if (k === " ") { e.preventDefault(); togglePlay(); }
  else if (k === "arrowleft") { e.preventDefault(); video.currentTime = Math.max(0, video.currentTime - (e.shiftKey ? 1 : 5)); }
  else if (k === "arrowright") { e.preventDefault(); video.currentTime = Math.min(video.duration || 0, video.currentTime + (e.shiftKey ? 1 : 5)); }
  else if (k === "i") setSel("start");
  else if (k === "o") setSel("end");
});

// ---------- clips tab ----------

let allClips = [];
async function loadClips() {
  allClips = await invoke("list_clips");
  renderClips();
}
for (const b of $("clip-filter").children) {
  b.addEventListener("click", () => {
    state.clipFilter = b.dataset.filter;
    for (const x of $("clip-filter").children) x.classList.toggle("active", x === b);
    renderClips();
  });
}

function renderClips() {
  const grid = $("clip-grid");
  const clips = allClips.filter((c) => state.clipFilter === "all" || (state.clipFilter === "auto") === !!c.auto);
  if (!clips.length) {
    grid.replaceChildren(h("div", { class: "empty" }, allClips.length ? "No clips match this filter." : "No clips yet. They appear here after a match, or when you save one from a recording."));
    return;
  }
  grid.replaceChildren(...clips.map((c) => {
    const thumb = h("div", { class: "thumb" + (c.thumbnail_path ? "" : " none") }, h("span", { class: "badge" }, Math.round(c.end - c.start) + " s"));
    if (c.thumbnail_path) thumb.style.backgroundImage = `url("${fileUrl(c.thumbnail_path)}")`;
    return h("div", { class: "clip-card", onclick: () => openClipPlayer(c) },
      thumb,
      h("div", { class: "meta" },
        h("div", { class: "grow" },
          h("div", { class: "name" }, c.title || "Clip"),
          h("div", { class: "sub" }, dateText(c.created_at))),
        c.auto && h("span", { class: "tag" }, "auto")));
  }));
}

/** Large player for a single clip, with its actions. */
function openClipPlayer(c) {
  const v = h("video", { src: fileUrl(c.path), controls: true, autoplay: true });
  const after = async () => { closeModal(); await loadClips(); };
  const acts = clipActions(c, after);
  acts.style.opacity = "1";
  openModal(true, v,
    h("div", { style: "display:flex;align-items:center;gap:10px;padding:0 4px" },
      h("div", { class: "grow", style: "flex:1;min-width:0" }, h("div", { class: "name", style: "font-weight:600" }, c.title || "Clip"), h("div", { class: "sub" }, dateText(c.created_at))),
      acts,
      h("button", { onclick: closeModal }, "Close")));
}

// ---------- settings ----------

async function loadSettings() {
  const [settings, info] = await Promise.all([invoke("get_settings"), invoke("app_info")]);
  state.settings = settings;
  state.padBefore = settings.auto_clips.pad_before;
  state.padAfter = settings.auto_clips.pad_after;
  renderSettings(info);
}

function renderSettings(info) {
  const s = state.settings;
  const a = s.auto_clips;
  const check = (id, checked) => h("input", { type: "checkbox", id, checked });
  const num = (id, value, min, max, step) => h("input", { type: "number", id, value, min, max, step: step || 1 });
  const field = (label, hint, control, indent) => h("div", { class: "field" + (indent ? " indent" : "") },
    h("label", { class: "name", htmlFor: control.id }, label, hint && h("span", { class: "hint" }, hint)), control);
  const card = (title, ...fields) => h("div", { class: "card" }, h("header", {}, h("h3", {}, title)), h("div", { class: "body" }, ...fields));

  const fps = h("select", { id: "s-fps" }, h("option", { value: 60, selected: s.fps === 60 }, "60 fps"), h("option", { value: 30, selected: s.fps === 30 }, "30 fps"));
  const form = $("settings-form");
  form.replaceChildren(
    card("Recording",
      field("Record matches automatically", "Starts when the game has loaded and stops when it ends.", check("s-record", s.record_enabled)),
      field("Frame rate", null, fps),
      field("Video quality (Mbps)", "Higher looks better and uses more disk. 20 Mbps is about 9 GB per hour.", num("s-bitrate", s.bitrate_kbps / 1000, 1, 100, 1))),
    card("Automatic clips",
      field("Make clips automatically after each match", null, check("s-auto", a.enabled)),
      field("My kills", "Multikills become one clip.", check("s-kills", a.kills), true),
      field("My deaths", null, check("s-deaths", a.deaths), true),
      field("My assists", null, check("s-assists", a.assists), true),
      field("Objectives my team takes", "Dragon, Baron, Herald, turrets, inhibitors.", check("s-objectives", a.objectives), true),
      field("Seconds before the moment", null, num("s-before", a.pad_before, 0, 120)),
      field("Seconds after the moment", null, num("s-after", a.pad_after, 0, 120))),
    card("Storage",
      field("Recordings folder", info.data_folder, h("button", { type: "button", id: "s-folder", onclick: () => invoke("open_data_folder").catch(fail) }, "Open folder")),
      field("Delete full recordings after (days)", "0 keeps them forever. Clips are never deleted automatically.", num("s-days", s.retention_days, 0, 3650)),
      field("Keep at most (GB) of full recordings", "0 means no limit. The oldest go first.", num("s-gb", s.max_recordings_gb, 0, 100000))),
    card("Hotkey and tools",
      field("Mark a moment while playing", "Adds a marker to the timeline. Examples: F9, Ctrl+F8, Alt+M.", h("input", { type: "text", id: "s-mark", value: s.hotkeys.mark })),
      field("ffmpeg location", info.ffmpeg_found ? "ffmpeg was found. Leave empty to find it automatically." : "ffmpeg was NOT found: recordings cannot be finalized or cut. Install it or enter the path to ffmpeg.exe.",
        h("input", { type: "text", id: "s-ffmpeg", value: s.ffmpeg_path || "", placeholder: "automatic" }))),
    h("div", { class: "save-bar" }, h("span", { class: "sub" }, "Recording settings apply from the next match."), h("button", { class: "primary", type: "submit" }, "Save settings")));
}

$("settings-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const s = structuredClone(state.settings); // keeps fields this screen does not show
  const n = (id) => Number($(id).value);
  s.record_enabled = $("s-record").checked;
  s.fps = n("s-fps");
  s.bitrate_kbps = Math.round(n("s-bitrate") * 1000);
  s.auto_clips = { ...s.auto_clips, enabled: $("s-auto").checked, kills: $("s-kills").checked, deaths: $("s-deaths").checked, assists: $("s-assists").checked, objectives: $("s-objectives").checked, pad_before: n("s-before"), pad_after: n("s-after") };
  s.retention_days = Math.round(n("s-days"));
  s.max_recordings_gb = Math.round(n("s-gb"));
  s.hotkeys = { ...s.hotkeys, mark: $("s-mark").value.trim() || "F9" };
  s.ffmpeg_path = $("s-ffmpeg").value.trim() || null;
  try {
    const warning = await invoke("save_settings", { settings: s });
    if (warning) toast(warning, true); else toast("Settings saved");
    await loadSettings();
  } catch (err) { fail(err); }
});

// ---------- start ----------

window.addEventListener("focus", () => {
  if (state.tab === "matches") loadMatches().catch(fail);
  if (state.tab === "clips") loadClips().catch(fail);
});

(async () => {
  try {
    const settings = await invoke("get_settings");
    state.padBefore = settings.auto_clips.pad_before;
    state.padAfter = settings.auto_clips.pad_after;
  } catch (_) { /* defaults are fine */ }
  await pollStatus();
  await loadMatches().catch(fail);
})();
