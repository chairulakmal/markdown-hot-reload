(() => {
  "use strict";

  const content = document.getElementById("content");

  // mermaid.min.js is 3.5MB and most documents contain no diagrams, so it is
  // embedded in the binary but only parsed once a diagram actually appears.
  let mermaidLoaded = null;

  const loadMermaid = () =>
    (mermaidLoaded ??= new Promise((resolve, reject) => {
      const script = document.createElement("script");
      script.src = "mermaid.min.js";
      script.onload = () => resolve(window.mermaid);
      script.onerror = () => reject(new Error("mermaid failed to load"));
      document.head.appendChild(script);
    }));

  async function drawDiagrams() {
    const blocks = content.querySelectorAll("pre.mermaid:not([data-drawn])");
    if (blocks.length === 0) return;

    const mermaid = await loadMermaid();
    // Re-applied on every pass, not just once at load: the rest of the page
    // follows prefers-color-scheme live through CSS, and a diagram frozen at
    // whatever theme was active on first draw would visibly mismatch it after
    // an OS theme switch.
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      theme: matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "default",
    });
    for (const block of blocks) {
      // Saved before the first render, because that render replaces the
      // block's text (the diagram source) with the rendered SVG markup, and
      // a theme-change redraw needs the original source back.
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
  }

  // The rest of the page follows prefers-color-scheme live through CSS; an
  // already-drawn diagram is a static SVG with colors baked in, so it needs
  // an explicit redraw to keep up with an OS theme switch that isn't paired
  // with a file save.
  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
    for (const block of content.querySelectorAll("pre.mermaid[data-drawn]")) {
      delete block.dataset.drawn;
    }
    void drawDiagrams();
  });

  // Morphing rather than replacing innerHTML is what preserves scroll position,
  // open <details> elements, and text selection across a reload.
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
