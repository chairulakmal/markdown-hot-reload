(() => {
  "use strict";

  // Application chrome: the view controls, kept out of app.js so that file
  // stays the render handshake and the Mermaid draw. Nothing here parses
  // markdown, unescapes HTML, or touches the network; the controls read and
  // write presentation state only.
  //
  // This file defines no global on the reload handshake's reserved prefix,
  // which belongs to INIT_SCRIPT and app.js alone; the handshake test in
  // main.rs asserts that. Chrome state stays in this closure.

  const content = document.getElementById("content");
  const overlay = document.getElementById("overlay");
  const panel = document.getElementById("overlay-panel");
  const readout = document.getElementById("readout");

  // ---------------------------------------------------------------- zoom

  // Discrete steps rather than a multiplier, so the levels are round numbers, a
  // person lands on the same ones every time, and repeated zooming out cannot
  // creep toward an unreadable fraction.
  const ZOOM_STEPS = [0.5, 0.67, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3];
  const ZOOM_DEFAULT = 1;
  const ZOOM_KEY = "mhr-zoom";

  // localStorage is the webview's own store, not a file this app writes, so
  // persisting here does not weaken the read-only promise. Every access is
  // guarded: a webview with site data disabled throws on the property itself,
  // not on the call, so a bare read would take the whole script down.
  function readStoredZoom() {
    try {
      return normalizeZoom(parseFloat(localStorage.getItem(ZOOM_KEY)));
    } catch {
      return ZOOM_DEFAULT;
    }
  }

  function storeZoom(zoom) {
    try {
      localStorage.setItem(ZOOM_KEY, String(zoom));
    } catch {
      // Nothing to do and nothing to say: zoom still applies for this window,
      // it just will not survive a restart.
    }
  }

  // The store outlives any one version of this app and is editable from the
  // webview's own inspector, so a value arriving here can be missing, a string
  // that is not a number, or outside the range the steps cover.
  function normalizeZoom(zoom) {
    if (!Number.isFinite(zoom)) return ZOOM_DEFAULT;
    const lowest = ZOOM_STEPS[0];
    const highest = ZOOM_STEPS[ZOOM_STEPS.length - 1];
    return Math.min(Math.max(zoom, lowest), highest);
  }

  let zoom = readStoredZoom();

  function setZoomProperty(value) {
    document.documentElement.style.setProperty("--mhr-zoom", String(value));
  }

  function applyZoom(next) {
    zoom = normalizeZoom(next);
    setZoomProperty(zoom);
    storeZoom(zoom);
  }

  // Steps to the neighbouring level. A stored value between two steps, whether
  // from an older build or typed into the inspector, moves to the next step in
  // the requested direction rather than snapping to the nearest one first,
  // which would make the first press appear to do nothing.
  function stepZoom(direction) {
    const next =
      direction > 0
        ? ZOOM_STEPS.find((step) => step > zoom)
        : ZOOM_STEPS.filter((step) => step < zoom).pop();
    applyZoom(next ?? zoom);
  }

  // INIT_SCRIPT already wrote the stored level before the first paint, from a
  // coarser bounds check than normalizeZoom. Rewriting the property here
  // repairs it if what was stored was out of range or unparseable. It does not
  // go back to the store, because opening the app is not a change and should
  // not write anything.
  setZoomProperty(zoom);

  // ------------------------------------------------------------- overlay

  // Focus moves into the panel so the arrow keys scroll the shortcut list
  // rather than the document behind it, and returns to wherever it was so
  // closing the overlay does not silently change what the next key reaches.
  let focusBeforeOverlay = null;

  function openOverlay() {
    if (!overlay.hidden) return;
    focusBeforeOverlay = document.activeElement;
    overlay.hidden = false;
    panel.focus();
  }

  function closeOverlay() {
    if (overlay.hidden) return;
    overlay.hidden = true;
    if (focusBeforeOverlay instanceof HTMLElement) focusBeforeOverlay.focus();
    focusBeforeOverlay = null;
  }

  function toggleOverlay() {
    if (overlay.hidden) openOverlay();
    else closeOverlay();
  }

  // Clicking the backdrop closes, clicking the panel does not. The check is on
  // the target rather than on a bounds test, so it stays correct whatever the
  // panel's size and position become.
  overlay.addEventListener("click", (event) => {
    if (event.target === overlay) closeOverlay();
  });

  // ------------------------------------------------------------ readout

  // What a link will actually do, shown before the click rather than after it.
  // This matters more here than on the web: an outbound click leaves for the
  // system browser and a local one is inert, so without a readout the only way
  // to learn where a link points is to follow it.
  //
  // Every same-page link resolves against this document's own URL, so that
  // prefix is stripped: a table-of-contents entry should read as "#a-heading"
  // rather than burying it after mhr://localhost/index.html. Taken from
  // location.href rather than location.origin, which is "null" for a custom
  // scheme in some webviews, and read once at load because a fragment jump
  // rewrites location.href afterwards.
  const base = location.href.split("#")[0];

  function linkAt(node) {
    return node && node.closest ? node.closest("a[href]") : null;
  }

  function showLink(anchor) {
    if (!anchor) {
      readout.hidden = true;
      return;
    }
    const href = anchor.href;
    // textContent, never innerHTML. Reading an attribute and writing it as
    // text is neither parsing nor unescaping, so the frontend invariant holds;
    // writing it as markup would put a document-controlled string back on the
    // page as HTML, which is exactly what render::sanitize exists to prevent.
    readout.textContent = href.startsWith(base)
      ? href.slice(base.length)
      : href;
    readout.hidden = false;
  }

  // Delegated to #content rather than bound per link. A reload morphs the
  // contents of #content, so a listener on an individual anchor is lost on the
  // next save, while one on #content itself survives every reload.
  //
  // mouseout reads relatedTarget, the element the pointer moved to. Moving
  // between two nodes inside one link therefore re-shows that same link
  // instead of hiding and immediately re-showing it.
  content.addEventListener("mouseover", (event) =>
    showLink(linkAt(event.target)),
  );
  content.addEventListener("mouseout", (event) =>
    showLink(linkAt(event.relatedTarget)),
  );

  // ------------------------------------------------------------- keys

  // The one keydown listener for the whole app. A control adds a row here
  // rather than a listener of its own, so the bindings read as one table and
  // two controls wanting the same key collide visibly instead of both firing.
  //
  // A row's key is the physical key (event.code), not the character
  // (event.key), prefixed with C for Ctrl or Cmd and S for Shift. event.code
  // names a position on the keyboard, so Ctrl with the key left of Backspace
  // is zoom in on every layout, while the character that key produces is "="
  // on some and something else on others. Cmd is folded into C because it is
  // the modifier a person expects for zoom on macOS.
  //
  // Both the shifted and unshifted forms of the zoom keys are bound, because
  // "zoom in" is Ctrl+= to some people and Ctrl+Shift+= to others, and the
  // numeric keypad is bound because that is where a person with a full-size
  // keyboard reaches for it.
  const byCode = new Map([
    ["C-Equal", () => stepZoom(1)],
    ["CS-Equal", () => stepZoom(1)],
    ["C-NumpadAdd", () => stepZoom(1)],
    ["C-Minus", () => stepZoom(-1)],
    ["CS-Minus", () => stepZoom(-1)],
    ["C-NumpadSubtract", () => stepZoom(-1)],
    ["C-Digit0", () => applyZoom(ZOOM_DEFAULT)],
    ["C-Numpad0", () => applyZoom(ZOOM_DEFAULT)],
    // Escape declines the key whenever nothing is open, so in the ordinary
    // case it still belongs to the webview rather than to this app.
    [
      "-Escape",
      () => {
        if (overlay.hidden) return false;
        closeOverlay();
        return true;
      },
    ],
  ]);

  // Checked only when no physical binding matched. "?" sits on a different
  // physical key on every layout, and it is the character the overlay itself
  // promises, so it is the one control matched by what was typed rather than
  // by where it was typed. Nothing here takes Ctrl, so it cannot collide with
  // a zoom row above.
  const byChar = new Map([["?", toggleOverlay]]);

  document.addEventListener("keydown", (event) => {
    if (event.altKey) return;
    const ctrl = event.ctrlKey || event.metaKey ? "C" : "";
    const shift = event.shiftKey ? "S" : "";
    const handler =
      byCode.get(`${ctrl}${shift}-${event.code}`) ??
      (ctrl ? undefined : byChar.get(event.key));
    if (!handler) return;
    // A handler returns false to decline the key it was offered. Only a key
    // this app actually acted on is taken, so the webview keeps its own
    // handling of every other one, and for the zoom keys that is what stops
    // WebKitGTK applying its page zoom on top of ours.
    if (handler(event) === false) return;
    event.preventDefault();
  });
})();
