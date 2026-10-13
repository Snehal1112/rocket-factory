"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const patch = require("../static/patch.js");

let writes = [];

class TextNode {
  constructor(value) {
    this.nodeType = 3;
    this.nodeName = "#text";
    this.value = value;
  }
  get nodeValue() {
    return this.value;
  }
  set nodeValue(next) {
    writes.push(`text:${next}`);
    this.value = next;
  }
}

class ElementNode {
  constructor(tag, attrs, kids) {
    this.nodeType = 1;
    this.nodeName = tag.toUpperCase();
    this.attrs = new Map(Object.entries(attrs || {}));
    this.childNodes = (kids || []).slice();
  }
  get attributes() {
    return [...this.attrs].map(([name, value]) => ({ name, value }));
  }
  getAttribute(name) {
    return this.attrs.has(name) ? this.attrs.get(name) : null;
  }
  setAttribute(name, value) {
    writes.push(`attr:${name}=${value}`);
    this.attrs.set(name, String(value));
  }
  removeAttribute(name) {
    writes.push(`unattr:${name}`);
    this.attrs.delete(name);
  }
  appendChild(node) {
    writes.push("append");
    this.childNodes.push(node);
  }
  removeChild(node) {
    writes.push("remove");
    this.childNodes.splice(this.childNodes.indexOf(node), 1);
  }
  replaceChild(next, old) {
    writes.push("replace");
    this.childNodes[this.childNodes.indexOf(old)] = next;
  }
}

const el = (tag, attrs, kids) => new ElementNode(tag, attrs, kids);
const txt = (value) => new TextNode(value);

function run(live, next) {
  writes = [];
  patch.patchChildren(live, next);
  return writes;
}

test("identical trees cause no writes", () => {
  const make = () => el("div", {}, [el("p", { class: "a" }, [txt("hello")]), txt("x")]);
  assert.deepEqual(run(make(), make()), []);
});

test("a changed text node is the only write", () => {
  const live = el("div", {}, [el("p", { class: "a" }, [txt("1m in stage")])]);
  const next = el("div", {}, [el("p", { class: "a" }, [txt("2m in stage")])]);
  assert.deepEqual(run(live, next), ["text:2m in stage"]);
});

test("a changed or removed attribute is patched in place", () => {
  const live = el("div", {}, [el("p", { class: "a", title: "t" }, [])]);
  const next = el("div", {}, [el("p", { class: "b" }, [])]);
  assert.deepEqual(run(live, next), ["attr:class=b", "unattr:title"]);
});

test("extra new children are appended and extra old children removed", () => {
  const live = el("div", {}, [el("p", {}, [])]);
  assert.deepEqual(run(live, el("div", {}, [el("p", {}, []), el("p", {}, [])])), ["append"]);
  assert.deepEqual(run(live, el("div", {}, [])), ["remove", "remove"]);
  assert.equal(live.childNodes.length, 0);
});

test("a different tag replaces the node and an unchanged element object is kept", () => {
  const live = el("div", {}, [el("p", {}, [txt("a")])]);
  assert.deepEqual(run(live, el("div", {}, [el("span", {}, [txt("a")])])), ["replace"]);
  const button = el("button", { "data-copy": "x" }, [txt("x")]);
  const tree = el("div", {}, [el("article", {}, [el("p", {}, [txt("old")]), button])]);
  run(tree, el("div", {}, [el("article", {}, [el("p", {}, [txt("new")]), el("button", { "data-copy": "x" }, [txt("x")])])]));
  assert.equal(tree.childNodes[0].childNodes[1], button);
});

test("a style attribute set from script survives a patch", () => {
  const live = el("div", {}, [el("span", { class: "f", style: "width: 50%;" }, [])]);
  assert.deepEqual(run(live, el("div", {}, [el("span", { class: "f" }, [])])), []);
});

test("an open details element stays open when the next render lacks the attribute", () => {
  const live = el("div", {}, [el("details", { class: "facts-box", open: "" }, [])]);
  const next = el("div", {}, [el("details", { class: "facts-box" }, [])]);
  assert.deepEqual(run(live, next), []);
  assert.equal(live.childNodes[0].getAttribute("open"), "");
});

test("a closed details element is opened when the next render has the attribute", () => {
  const live = el("div", {}, [el("details", { class: "agents-box" }, [])]);
  const next = el("div", {}, [el("details", { class: "agents-box", open: "" }, [])]);
  assert.deepEqual(run(live, next), ["attr:open="]);
});
