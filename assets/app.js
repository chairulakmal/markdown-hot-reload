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

  // Mermaid names its light palette "default", not "light". An override set by
  // chrome.js wins over the OS preference; with no override the OS decides.
  function mermaidTheme() {
    const override = document.documentElement.dataset.theme;
    if (override === "dark" || override === "light") {
      return override === "dark" ? "dark" : "default";
    }
    return matchMedia("(prefers-color-scheme: dark)").matches
      ? "dark"
      : "default";
  }

  // drawDiagrams awaits (mermaid load, each render), and a reload or a theme
  // switch can call it again mid-flight. Without this guard both passes see
  // the same :not([data-drawn]) blocks and render each twice.
  let drawing = false;
  let drawAgain = false;
  // Set by the theme listener instead of having it clear data-drawn directly.
  // A theme switch arriving mid-pass has to invalidate the blocks that pass is
  // still drawing with the old theme, and it cannot know which those are; the
  // next pass reads this flag and clears every one of them.
  let themeChanged = false;
  // Bumped by render() before it morphs. A pass captures it and abandons after
  // any await where it no longer matches, because the blocks that pass is
  // holding have been morphed to new source: writing the resolved SVG into one
  // would put a diagram from the previous document on the page, and marking it
  // drawn would hide it from every later pass.
  let generation = 0;

  async function drawDiagrams() {
    if (drawing) {
      drawAgain = true;
      return;
    }

    drawing = true;
    try {
      do {
        drawAgain = false;
        const pass = generation;

        if (themeChanged) {
          themeChanged = false;
          for (const drawn of content.querySelectorAll(
            "pre.mermaid[data-drawn]",
          )) {
            delete drawn.dataset.drawn;
          }
        }

        const blocks = content.querySelectorAll("pre.mermaid:not([data-drawn])");
        if (blocks.length === 0) continue;

        const mermaid = await loadMermaid();
        if (pass !== generation) {
          drawAgain = true;
          continue;
        }

        // Re-applied on every pass rather than once at load, so a diagram picks
        // up whichever theme is current instead of the one active the first time
        // any diagram was ever drawn.
        mermaid.initialize({
          startOnLoad: false,
          securityLevel: "strict",
          theme: mermaidTheme(),
        });
        for (const block of blocks) {
          // The first render overwrites the block's text with the rendered SVG,
          // so the source is cached here for any later redraw to reuse.
          const source = block.dataset.source ?? block.textContent;
          block.dataset.source = source;
          let svg = null;
          let failure = null;
          try {
            ({ svg } = await mermaid.render(
              `mermaid-${Math.random().toString(36).slice(2)}`,
              source,
            ));
          } catch (error) {
            failure = String(error);
          }
          // Checked after the await and before the write, so an abandoned block
          // keeps no data-drawn and the next pass picks it up. A block that
          // failed to render was still handled, so it is marked drawn either
          // way and is not retried until the source changes.
          if (pass !== generation) {
            drawAgain = true;
            break;
          }
          if (failure === null) {
            block.innerHTML = svg;
          } else {
            block.dataset.error = "1";
            block.textContent = failure;
          }
          block.dataset.drawn = "1";
        }
      } while (drawAgain || themeChanged);
    } finally {
      drawing = false;
    }
  }

  // A drawn diagram is a static SVG with colors baked in, so a theme switch
  // alone won't repaint it: mark every diagram stale and redraw. The OS query
  // covers a change with no override in force; the event covers an override
  // applied by chrome.js, which the query never sees.
  const redrawForTheme = () => {
    themeChanged = true;
    void drawDiagrams();
  };
  matchMedia("(prefers-color-scheme: dark)").addEventListener(
    "change",
    redrawForTheme,
  );
  document.addEventListener("mhr:themechange", redrawForTheme);

  // A drawn diagram whose source did not change keeps its SVG. Left to the
  // morph, the new markup strips data-drawn and swaps the SVG back to the
  // source text, so every save redraws every diagram and each one flashes
  // its source first. Comparing two strings is neither parsing nor
  // unescaping, so the frontend invariant holds.
  function keepsDrawnDiagram(oldNode, newNode) {
    return (
      oldNode instanceof Element &&
      newNode instanceof Element &&
      oldNode.matches("pre.mermaid[data-drawn]") &&
      newNode.matches("pre.mermaid") &&
      oldNode.dataset.source === newNode.textContent
    );
  }

  // Morphing (not replacing innerHTML) preserves scroll position, open
  // <details> elements, and text selection across a reload.
  function render(html) {
    generation += 1;
    Idiomorph.morph(content, html, {
      morphStyle: "innerHTML",
      callbacks: {
        beforeNodeMorphed: (oldNode, newNode) =>
          !keepsDrawnDiagram(oldNode, newNode),
      },
    });
    void drawDiagrams();
  }

  const queued = window.__q || [];
  window.__q = [];
  window.__render = render;

  for (const html of queued) render(html);
  void drawDiagrams();
})();
