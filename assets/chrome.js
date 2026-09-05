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

  // The one keydown listener for the whole app. A control adds a row here
  // rather than a listener of its own, so the bindings read as one table and
  // two controls wanting the same key collide visibly instead of both firing.
  //
  // A row's key is the physical key (event.code), not the character
  // (event.key), prefixed with C for Ctrl or Cmd and S for Shift. Ctrl+= and
  // Ctrl+- need a shift on several non-US layouts, where matching on the
  // character silently stops working. Cmd is folded into C because it is the
  // modifier a person expects for zoom on macOS.
  //
  // The table is empty because the controls land next: the help overlay on
  // "S-Slash" and "-Escape", the theme cycle on "-KeyT", and zoom on
  // "C-Equal", "C-Minus" and "C-Digit0".
  const bindings = new Map();

  document.addEventListener("keydown", (event) => {
    if (event.altKey) return;
    const ctrl = event.ctrlKey || event.metaKey ? "C" : "";
    const shift = event.shiftKey ? "S" : "";
    const handler = bindings.get(`${ctrl}${shift}-${event.code}`);
    if (!handler) return;
    // Only for a key this app claims, so the webview keeps its own handling of
    // every other key.
    event.preventDefault();
    handler(event);
  });
})();
