(() => {
  "use strict";

  const content = document.getElementById("content");

  // 3.5MB and most documents have no diagrams, so it loads lazily on first
  // use rather than embedding that cost into every page load.
  let mermaidLoaded = null;

  const loadMermaid = () =>
    (mermaidLoaded ??= new Promise((resolve, reject) => {
      const script = document.createElement("script");
      script.src = "mermaid.min.js";
      script.onload = () => resolve(window.mermaid);
      script.onerror = () => reject(new Error("mermaid failed to load"));
      document.head.appendChild(script);
    }));

  // drawDiagrams awaits (mermaid load, each render), and a reload can call it
  // again mid-flight. Without this guard both passes see the same
  // :not([data-drawn]) blocks and render each twice.
  let drawing = false;
  let drawAgain = false;

  async function drawDiagrams() {
    if (drawing) {
      drawAgain = true;
      return;
    }
    const blocks = content.querySelectorAll("pre.mermaid:not([data-drawn])");
    if (blocks.length === 0) return;

    drawing = true;
    try {
      const mermaid = await loadMermaid();
      // Re-applied on every pass rather than once at load, so a diagram picks
      // up whichever theme is current instead of the one active the first time
      // any diagram was ever drawn.
      mermaid.initialize({
        startOnLoad: false,
        securityLevel: "strict",
        theme: matchMedia("(prefers-color-scheme: dark)").matches
          ? "dark"
          : "default",
      });
      for (const block of blocks) {
        // The first render overwrites the block's text with the rendered SVG,
        // so the source is cached here for any later redraw to reuse.
        const source = block.dataset.source ?? block.textContent;
        block.dataset.source = source;
        block.dataset.drawn = "1";
        try {
          const { svg } = await mermaid.render(
            `mermaid-${Math.random().toString(36).slice(2)}`,
            source,
          );
          block.innerHTML = svg;
        } catch (error) {
          block.dataset.error = "1";
          block.textContent = String(error);
        }
      }
    } finally {
      drawing = false;
    }
    if (drawAgain) {
      drawAgain = false;
      void drawDiagrams();
    }
  }

  // A drawn diagram is a static SVG with colors baked in, so an OS theme
  // switch alone won't repaint it: clear data-drawn and redraw.
  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
    for (const block of content.querySelectorAll("pre.mermaid[data-drawn]")) {
      delete block.dataset.drawn;
    }
    void drawDiagrams();
  });

  // Morphing (not replacing innerHTML) preserves scroll position, open
  // <details> elements, and text selection across a reload.
  function render(html) {
    Idiomorph.morph(content, html, { morphStyle: "innerHTML" });
    void drawDiagrams();
  }

  const queued = window.__q || [];
  window.__q = [];
  window.__render = render;

  for (const html of queued) render(html);
  void drawDiagrams();
})();
