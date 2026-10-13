"use strict";

// Morphs a live DOM subtree into a freshly rendered one and writes only what differs.
(function (root) {
  function syncAttributes(live, next) {
    const wanted = Array.from(next.attributes);
    for (const attr of wanted) {
      if (live.getAttribute(attr.name) !== attr.value) live.setAttribute(attr.name, attr.value);
    }
    const names = new Set(wanted.map((attr) => attr.name));
    for (const attr of Array.from(live.attributes)) {
      // The style attribute is set from script (token bar widths), so a patch keeps it.
      if (attr.name === "style" && !names.has(attr.name)) continue;
      // A details element the user opened must stay open across the redraw.
      if (attr.name === "open" && live.nodeName === "DETAILS") continue;
      if (!names.has(attr.name)) live.removeAttribute(attr.name);
    }
  }

  function patchNode(parent, live, next) {
    if (live.nodeType !== next.nodeType || live.nodeName !== next.nodeName) {
      parent.replaceChild(next, live);
      return;
    }
    if (live.nodeType === 3) {
      if (live.nodeValue !== next.nodeValue) live.nodeValue = next.nodeValue;
      return;
    }
    if (live.nodeType === 1) {
      syncAttributes(live, next);
      patchChildren(live, next);
    }
  }

  function patchChildren(liveParent, nextParent) {
    const live = Array.from(liveParent.childNodes);
    const next = Array.from(nextParent.childNodes);
    for (let i = 0; i < next.length; i++) {
      if (i < live.length) patchNode(liveParent, live[i], next[i]);
      else liveParent.appendChild(next[i]);
    }
    for (let i = live.length - 1; i >= next.length; i--) liveParent.removeChild(live[i]);
  }

  const api = { patchChildren };
  if (typeof module !== "undefined" && module.exports) {
    module.exports = api;
  } else {
    root.FactoryPatch = api;
  }
})(typeof window !== "undefined" ? window : globalThis);
