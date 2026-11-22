const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const read = file => fs.readFileSync(path.join(__dirname, '..', file), 'utf8');
const html = read('ui/src/popover/settings.html');
const categories = ['general', 'devices', 'sound', 'talk', 'shortcuts', 'notifications', 'appearance', 'advanced', 'updates', 'about'];
for (const key of categories) {
  assert.ok(html.includes('data-category="' + key + '"'));
  assert.ok(html.includes('data-content="' + key + '"'));
}
const ids = [...html.matchAll(/id="([^"]+)"/g)].map(match => match[1]);
assert.equal(ids.length, new Set(ids).size);
const updateScript = read('ui/src/popover/update-settings.js');
const js = read('ui/src/popover/winx-settings.js') + updateScript;
for (const [, id] of js.matchAll(/(?:getElementById|[$])\('([^']+)'\)/g)) assert.ok(ids.includes(id), id);
assert.ok(!read('ui/src/popover/index.html').includes('view-winx-settings'));
assert.ok(read('src/panel.rs').includes('WinXSettingsWindow'));
assert.ok(read('src/main.rs').includes('Event::SettingsIpc'));
for (const name of ['Threalcocoboy', 'Nipun Yatawara', 'Yash Raut', 'Nothing Technology Limited']) assert.ok(html.includes(name));
assert.ok(read('packaging/windows.nsi').includes('"Publisher" "Threalcocoboy"'));
assert.ok(read('build.rs').includes('"CompanyName", "Threalcocoboy"'));
console.log('Settings categories, native wiring, recovery controls, and WinX attribution passed.');

async function checkUpdates() {
  const listeners = new Map();
  const elements = Object.fromEntries(ids.map(id => [id, {
    id,
    addEventListener(type, handler) { listeners.set(id + ':' + type, handler); },
    focus() { focused = id; },
  }]));
  elements['update-consent'].hidden = true;
  const options = ['automatic', 'notify', 'prereleases'].map(key => ({
    dataset: { updateOption: key },
    addEventListener(type, handler) { listeners.set(key + ':' + type, handler); },
  }));
  let focused;
  let deliver;
  let opened = false;
  const state = {
    current: '1.5.0', latest: null, status: 'Waiting to check', available: false,
    notes: null, checked: null, busy: false, ready: false, downloading: false,
    received: 0, bytes: null, error: '', options: { automatic: true, notify: true, prereleases: false },
  };
  const api = {
    onUpdateState: handler => { deliver = handler; },
    updateSettings: async (key, value) => { state.options[key] = value; return state; },
    updateCheck: async () => Object.assign(state, { busy: true, status: 'Checking for updates' }),
    updateDownload: async () => Object.assign(state, { downloading: true, busy: true, received: 20 }),
    updateCancel: async () => Object.assign(state, { ready: false }),
    updateConfirm: async () => Object.assign(state, { ready: false, busy: true, status: 'Finishing Talk Mode recovery' }),
    updateRelease: () => { opened = true; },
  };
  const window = {};
  const document = {
    getElementById: id => elements[id],
    querySelectorAll: () => options,
    querySelector: () => ({ hidden: false }),
  };
  vm.runInNewContext(updateScript, { window, document });
  window.bindUpdateSettings(api, state);
  assert.ok(elements['update-download'].hidden);
  assert.equal(elements['update-current'].textContent, '1.5.0');
  assert.equal(elements['update-checked'].textContent, 'Never');
  assert.equal(options[2].checked, false);
  options[2].checked = true;
  await listeners.get('prereleases:change')();
  assert.equal(state.options.prereleases, true);
  await listeners.get('update-check:click')();
  assert.ok(elements['update-check'].disabled);
  assert.ok(options[2].disabled);
  Object.assign(state, { latest: '1.6.0', available: true, checked: 1_700_000_000, busy: false,
    status: 'Update available', bytes: 100, notes: '<b>Release notes remain text</b>' });
  deliver(state);
  assert.equal(elements['update-notes'].textContent, state.notes);
  assert.ok(!elements['update-download'].hidden);
  await listeners.get('update-download:click')();
  assert.equal(elements['update-progress'].value, 20);
  assert.equal(elements['update-size'].textContent, '20%');
  assert.ok(!elements['update-progress-group'].hidden);
  Object.assign(state, { busy: false, downloading: false, ready: true });
  deliver(state);
  assert.equal(focused, 'update-cancel');
  await listeners.get('update-cancel:click')();
  assert.ok(elements['update-consent'].hidden);
  assert.equal(focused, 'update-download');
  state.ready = true;
  deliver(state);
  await listeners.get('update-confirm:click')();
  assert.equal(elements['update-status'].textContent, 'Finishing Talk Mode recovery');
  Object.assign(state, { busy: false, status: 'Update failed', error: 'Talk Mode recovery is pending' });
  deliver(state);
  assert.ok(!elements['update-error'].hidden);
  listeners.get('update-release:click')();
  assert.ok(opened);
  console.log('Update settings, download progress, consent, recovery state, and release links passed.');
}

checkUpdates().catch(error => { console.error(error); process.exitCode = 1; });
