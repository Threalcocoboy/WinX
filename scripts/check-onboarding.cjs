const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

const ui = path.join(__dirname, '../ui/src/popover');
const markup = fs.readFileSync(path.join(ui, 'index.html'), 'utf8');
const nodes = new Map();
const selectors = new Map();
const calls = [];
const listeners = {};
const document = {
  activeElement: null,
  documentElement: element(),
  getElementById(id) { return nodes.get(id); },
  createElement() { return element(); },
  addEventListener(name, fn) { listeners[name] = fn; },
  hasFocus() { return true; },
};
function element() {
  const children = [];
  const attrs = new Map();
  return {
    children, dataset: {}, hidden: false, style: { setProperty() {} },
    append(...items) { children.push(...items); },
    prepend(...items) { children.unshift(...items); },
    replaceChildren(...items) { children.splice(0, children.length, ...items); },
    get firstElementChild() { return children[0]; },
    get lastElementChild() { return children.at(-1); },
    get childElementCount() { return children.length; },
    addEventListener(name, fn) { this.events ||= {}; this.events[name] = fn; },
    setAttribute(name, value) { attrs.set(name, value); },
    removeAttribute(name) { attrs.delete(name); },
    toggleAttribute(name, present) { if (present) attrs.set(name, ''); else attrs.delete(name); },
    focus() { document.activeElement = this; },
    querySelector(selector) {
      if (!selectors.has(selector)) selectors.set(selector, element());
      return selectors.get(selector);
    },
    querySelectorAll(selector) { return selectors.get(selector) || []; },
  };
}
for (const match of markup.matchAll(/id="([^"]+)"/g)) nodes.set(match[1], element());
const root = nodes.get('winx-tour');
root.hidden = true;
const get = id => nodes.get('tour-' + id);
get('search-state').append(element(), element());
const pages = [...markup.matchAll(/data-scene="([0-9])"/g)].map(() => element());
assert.equal(pages.length, 6);
selectors.set('[data-scene]', pages);
const audio = get('audio');
audio.paused = true;
audio.muted = false;
audio.play = async () => { calls.push(['play']); audio.paused = false; };
audio.pause = () => { audio.paused = true; };
const state = { phase: 'off', volume: null };
const prefs = {
  overlay: { position: 'bottomRight', media: {}, enabled: true },
  battery: { enabled: true },
  shortcuts: ['panel', 'cycleNoise', 'transparency', 'lowLatency', 'talkMode'].map(action => ({
    action, label: action, available: false, chord: { modifiers: 5, vk: 75 },
  })),
  hotkeysEnabled: true,
  talk: { enabled: true, fadeMs: 600 },
  talkStatus: state,
};
const api = {
  awarenessSettings: async () => prefs,
  setAwarenessSettings: async (...args) => { calls.push(args); return { settings: prefs }; },
  onboarding: async (...args) => { calls.push(args); return {}; },
  talkToggle: async () => { calls.push(['hardware']); return state; },
  onTalkState: fn => { listeners.talkState = fn; },
  onTourDevices: fn => { listeners.devices = fn; },
  onTourShortcut: fn => { listeners.nativeShortcut = fn; },
};
const context = vm.createContext({
  document, window: { matchMedia: () => ({ matches: true }), addEventListener() {} },
  setTimeout: () => 1, clearTimeout() {}, cancelAnimationFrame() {}, requestAnimationFrame() {},
});
for (const file of ['shortcut-keys.js', 'onboarding.js']) {
  vm.runInContext(fs.readFileSync(path.join(ui, file), 'utf8'), context);
}
assert.equal(context.window.winxKeys.name({ modifiers: 7, vk: 40 }), 'Ctrl + Alt + Shift + Down');
assert.equal(context.window.winxKeys.chordFor({ metaKey: true, ctrlKey: true, code: 'KeyH' }), null);
assert.equal(context.window.winxKeys.chordFor({ altKey: true, shiftKey: true, code: 'KeyK' }).vk, 75);
assert.equal(context.window.winxKeys.chordFor({ ctrlKey: true, code: 'F12' }), null);
const tour = context.window.createOnboarding({ api, symbols: { budLeft: '' }, talkStatus: state, onExit() {} });
const click = action => root.events.click({ target: { closest: () => ({ dataset: { tour: action } }) } });
(async () => {
  await tour.open();
  assert.equal(root.hidden, false);
  assert.equal(calls.filter(([action]) => action === 'play').length, 0);
  await click('next');
  listeners.devices({ ready: false, name: '', devices: [] });
  await click('skipDevice');
  await click('overlayPreview');
  await click('next');
  await click('next');
  assert.equal(get('demo-keys').children.map(item => item.textContent).join(' '), 'Alt Shift K');
  assert.equal(get('demo-start').disabled, true);
  await click('previewDemo');
  listeners.keydown({ code: 'AltLeft', altKey: true });
  listeners.keydown({ code: 'KeyK', altKey: true, shiftKey: true, repeat: false, preventDefault() {} });
  assert.match(get('demo-status').textContent, /Preview only/);
  listeners.keydown({ code: 'KeyK', altKey: true, shiftKey: true, repeat: false, preventDefault() {} });
  assert.match(get('demo-status').textContent, /Preview finished/);
  assert.ok(calls.some(command => command.join(' ') === 'preview ancOn'));
  assert.ok(!calls.some(([action, enable]) => action === 'hardware' || action === 'demo' && enable));
  assert.ok(!calls.some(([action]) => action === 'play'));
  await click('next');
  await click('finish');
  assert.equal(root.hidden, true);
  console.log('Six onboarding scenes, default-silent audio, overlay preview, alternate shortcuts, and isolated Talk Mode preview passed.');
})().catch(error => { console.error(error); process.exitCode = 1; });
