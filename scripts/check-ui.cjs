// Exercise battery/model rendering without hardware or a browser dependency.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const nodes = new Map();
function element() {
  const children = new Map();
  const classes = new Set();
  return {
    hidden: false, textContent: '', style: { setProperty() {} }, dataset: {},
    addEventListener(name, callback) { this[name] = callback; },
    setAttribute() {},
    querySelectorAll() { return []; },
    classList: {
      toggle(name, force) {
        if (force === undefined) {
          if (classes.has(name)) classes.delete(name);
          else classes.add(name);
        } else if (force) {
          classes.add(name);
        } else {
          classes.delete(name);
        }
      },
      contains(name) { return classes.has(name); },
    },
    querySelector(selector) {
      if (!children.has(selector)) children.set(selector, element());
      return children.get(selector);
    },
  };
}
const document = {
  documentElement: element(),
  getElementById(id) { return this.querySelector('#' + id); },
  querySelectorAll() { return []; },
  querySelector(selector) {
    if (!nodes.has(selector)) nodes.set(selector, element());
    return nodes.get(selector);
  },
};
const context = { document, window: {
  createSwitchLock: () => ({}),
  nx: { init: () => new Promise(() => {}) },
} };
vm.createContext(context);
vm.runInContext(fs.readFileSync(path.join(__dirname, '../ui/src/popover/renderer.js'), 'utf8'), context);
const bud = part => document.querySelector(`.bud[data-part="${part}"]`);
context.renderBattery({ stereo: { level: 72, charging: false }, left: null, right: null, case: null });
assert.equal(bud('stereo').hidden, false);
assert.equal(bud('stereo').querySelector('.pct').textContent, '72%');
assert.equal(bud('left').hidden, true);
assert.equal(bud('right').hidden, true);
context.renderBattery({ stereo: null, left: { level: 87 }, right: { level: 80 }, case: null });
assert.equal(bud('stereo').hidden, true);
assert.equal(bud('left').hidden, false);
assert.equal(bud('right').hidden, false);
assert.equal(bud('left').querySelector('.pct').textContent, '87%');
assert.equal(bud('case').hidden, true);
context.renderCapabilities({ noiseCancellation: false, strengths: [], adaptive: false, transparency: false });
assert.equal(document.querySelector('#noise-section').hidden, true);
context.renderCapabilities({ noiseCancellation: true, strengths: ['low', 'mid', 'high'], adaptive: true, transparency: true });
assert.equal(document.querySelector('#noise-section').hidden, false);
assert.equal(document.querySelector('.option[data-mode="anc"]').hidden, false);
context.renderCapabilities({ noiseCancellation: true, strengths: [], adaptive: false, transparency: true });
assert.equal(document.querySelector('.option[data-mode="anc"]').hidden, false);

// Fallback visibility and header states across idle, unrecognized, unresponsive, limited.
context.renderHeader({ connected: false, name: null, fallback: { mode: 'none', deviceName: null } });
context.renderFallback({ mode: 'none', deviceName: null });
assert.equal(document.querySelector('#fallback-card').hidden, true);
assert.equal(document.querySelector('#panel').classList.contains('fallback-full'), false);
assert.equal(document.querySelector('#device-name').textContent, 'No Earbuds');

context.renderHeader({ connected: false, name: null, fallback: { mode: 'unrecognized', deviceName: 'CMF Buds 3' } });
context.renderFallback({ mode: 'unrecognized', deviceName: 'CMF Buds 3' });
assert.equal(document.querySelector('#fallback-card').hidden, false);
assert.equal(document.querySelector('#panel').classList.contains('fallback-full'), true);
assert.equal(document.querySelector('#fallback-card').classList.contains('compact'), false);
assert.equal(document.querySelector('#device-name').textContent, 'CMF Buds 3');
assert.equal(document.querySelector('#device-status').textContent, 'Not recognized');

context.renderHeader({ connected: true, name: 'CMF Buds Pro 2', fallback: { mode: 'unresponsive', deviceName: 'CMF Buds Pro 2' } });
context.renderFallback({ mode: 'unresponsive', deviceName: 'CMF Buds Pro 2' });
assert.equal(document.querySelector('#fallback-card').hidden, false);
assert.equal(document.querySelector('#panel').classList.contains('fallback-full'), true);
assert.equal(document.querySelector('#device-status').textContent, 'No response');

context.renderHeader({ connected: true, name: 'CMF Buds 3 Pro', fallback: { mode: 'limited', deviceName: 'CMF Buds 3 Pro' } });
context.renderFallback({ mode: 'limited', deviceName: 'CMF Buds 3 Pro' });
assert.equal(document.querySelector('#fallback-card').hidden, false);
assert.equal(document.querySelector('#panel').classList.contains('fallback-full'), false);
assert.equal(document.querySelector('#fallback-card').classList.contains('compact'), true);
assert.equal(document.querySelector('#device-status').textContent, 'Connected · Basic');

// Footer update button states: idle, checking, available (blue), downloading, upToDate, error.
const footer = document.querySelector('#app-name');
context.renderUpdate({ status: 'idle', version: null, message: null });
assert.equal(footer.textContent, 'NothingX Desktop');
assert.equal(footer.classList.contains('update-available'), false);
assert.equal(footer.disabled, false);

context.renderUpdate({ status: 'checking', version: null, message: null });
assert.equal(footer.textContent, 'Checking for updates…');
assert.equal(footer.disabled, true);

context.renderUpdate({ status: 'available', version: '1.2', message: null });
assert.equal(footer.textContent, 'There is an update');
assert.equal(footer.classList.contains('update-available'), true);
assert.equal(footer.disabled, false);

context.renderUpdate({ status: 'downloading', version: '1.2', message: null });
assert.equal(footer.textContent, 'Downloading update…');
assert.equal(footer.disabled, true);

context.renderUpdate({ status: 'upToDate', version: null, message: null });
assert.equal(footer.textContent, 'Up to date');
assert.equal(footer.classList.contains('update-available'), false);

console.log('Battery, model-dependent controls, fallback modes, and update footer passed.');

async function checkWindowsConnectionUpdates() {
  const state = {
    connected: false, name: null, fallback: { mode: 'none' }, battery: {},
    capabilities: { ring: 'none' }, quick: { pending: {}, ringing: null },
  };
  const window = context.window;
  for (const factory of ['createStrengthControl', 'createBassControl', 'createSpatialAudioTile',
    'createDualConnectionTile', 'createControls', 'createEqualizer']) {
    window[factory] = () => ({ render() {} });
  }
  window.createViews = () => ({ onBack() {}, isOpen: () => false });
  window.createOnboarding = () => ({});
  context.ResizeObserver = class { observe() {} };
  vm.runInContext('switchLock.sync = () => {};', context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../ui/src/popover/quick-settings.js'), 'utf8'), context);
  let push;
  let openedSettings = false;
  Object.assign(window.nx, {
    init: async () => ({ device: state, symbols: {}, winxSettings: true }),
    onAccent() {}, onRoute() {}, onDeviceState(callback) { push = callback; },
    openSettings() { openedSettings = true; },
  });
  await context.main();
  assert.equal(document.getElementById('device-status').textContent, 'Not connected');
  assert.equal(document.getElementById('settings').disabled, false);
  document.getElementById('settings').click();
  assert.ok(openedSettings);
  push({ ...state, connected: true, name: 'Nothing Ear (3)', battery: { left: { level: 83 } } });
  assert.equal(document.getElementById('device-status').textContent, 'Connected');
  assert.equal(document.getElementById('device-name').textContent, 'Nothing Ear (3)');
  assert.equal(bud('left').querySelector('.pct').textContent, '83%');
  push(state);
  assert.equal(document.getElementById('device-status').textContent, 'Not connected');
  console.log('Windows main menu initializes disconnected and renders connection updates.');
}
checkWindowsConnectionUpdates().catch(error => { console.error(error); process.exitCode = 1; });
