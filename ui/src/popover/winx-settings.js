(async function initializeSettings(api) {
  const init = await api.init();
  const $ = id => document.getElementById(id);
  $('settings-version').textContent = init.version;
  $('settings-startup').checked = init.startup;
  const setAccent = color => document.documentElement.style.setProperty('--accent', color);
  setAccent(init.accent);
  api.onAccent(setAccent);
  for (const icon of document.querySelectorAll('[data-sym]')) {
    icon.style.setProperty('--sym', 'url("' + init.symbols[icon.dataset.sym] + '")');
  }
  const categories = [...document.querySelectorAll('[data-category]')];
  function navigate(category, focus = true) {
    for (const button of categories) {
      if (button.dataset.category === category) button.setAttribute('aria-current', 'page');
      else button.removeAttribute('aria-current');
    }
    for (const section of document.querySelectorAll('[data-content]')) {
      section.hidden = section.dataset.content !== category;
      if (!section.hidden && focus) section.querySelector('h1').focus();
    }
    $('settings-main').scrollTop = 0;
  }
  for (const button of categories) button.addEventListener('click', () => { cancelRecording(); navigate(button.dataset.category); });
  navigate(init.settingsRoute || 'general', false);
  api.onRoute(route => { if (categories.some(button => button.dataset.category === route)) navigate(route); });

  window.bindUpdateSettings(api, init.winxUpdate);
  const root = document.getElementById('view-winx-settings');
  const error = document.getElementById('winx-setting-error');
  const list = document.getElementById('winx-shortcuts');
  const quick = document.getElementById('winx-device-settings');
  const preview = document.getElementById('winx-preview-action');
  let settings;
  let device;
  let recording;
  let recordingTimeout;
  let serial = Promise.resolve();

  const { name: chordName, chordFor } = window.winxKeys;

  function report(message) {
    error.hidden = !message;
    error.textContent = message || '';
    if (message) error.scrollIntoView({ block: 'nearest' });
  }

  function submit(...args) {
    serial = serial.then(async () => {
      const result = await api.setAwarenessSettings(...args);
      settings = result.settings;
      render();
      report(result.error);
    }).catch(() => report('WinX could not save that setting.'));
  }

  function patch(group, key, value) {
    serial = serial.then(async () => {
      const next = { ...settings[group === 'media' ? 'overlay' : group] };
      if (typeof key === 'number') {
        next.thresholds = [...next.thresholds];
        next.thresholds[key] = value;
      } else if (group === 'media') {
        next.media = { ...next.media, [key]: value };
      } else {
        next[key] = value;
      }
      const result = await api.setAwarenessSettings(group === 'media' ? 'overlay' : group, next);
      settings = result.settings;
      render();
      report(result.error);
    }).catch(() => report('WinX could not save that setting.'));
  }

  function button(text, description, action) {
    const element = document.createElement('button');
    element.type = 'button';
    element.className = 'winx-button';
    element.textContent = text;
    if (description) element.setAttribute('aria-label', description);
    element.addEventListener('click', action);
    return element;
  }

  function renderShortcuts() {
    const lastFocused = document.activeElement?.dataset.shortcut;
    const rows = [];
    for (const { action, label, chord, available } of settings.shortcuts) {
      const entry = document.createElement('div');
      entry.className = 'winx-shortcut';
      const name = document.createElement('span');
      const status = document.createElement('small');
      const presetIndex = ['First', 'Second', 'Third', 'Fourth', 'Fifth', 'Sixth', 'Seventh', 'Eighth']
        .findIndex(suffix => action === 'eq' + suffix);
      const preset = presetIndex < 0 ? null : device?.capabilities.eq?.presets?.[presetIndex];
      name.textContent = preset ? 'Select ' + preset.label : label;
      status.textContent = recording === action ? 'Press a shortcut · Esc cancels'
        : chordName(chord) + (chord && settings.hotkeysEnabled && !available ? ' · Unavailable' : '');
      name.append(status);
      const edit = button(recording === action ? 'Cancel' : 'Edit', 'Edit ' + label, () => {
        if (recording === action) cancelRecording();
        else {
          recording = action;
          clearTimeout(recordingTimeout);
          recordingTimeout = setTimeout(cancelRecording, 19500);
          submit('recordShortcut', true);
          renderShortcuts();
        }
      });
      edit.dataset.shortcut = action;
      const clear = button('Clear', 'Clear ' + label, () => {
        cancelRecording();
        submit('shortcut', action, null);
      });
      clear.disabled = !chord;
      entry.append(name, edit, clear);
      rows.push(entry);
    }
    list.replaceChildren(...rows);
    if (lastFocused) list.querySelector('[data-shortcut="' + lastFocused + '"]')?.focus();
  }

  function render() {
    if (!settings) return;
    const monitor = root.querySelector('[data-overlay="monitor"]');
    const choices = [{ id: 'foreground', label: 'Foreground app' }, { id: 'primary', label: 'Primary monitor' }, ...settings.monitors];
    if (!choices.some(choice => choice.id === settings.overlay.monitor)) {
      choices.push({ id: settings.overlay.monitor, label: 'Saved display (offline)' });
    }
    monitor.replaceChildren(...choices.map(choice => new Option(choice.label, choice.id)));
    for (const group of ['overlay', 'battery', 'talk']) {
      for (const field of root.querySelectorAll('[data-' + group + ']')) {
        const value = settings[group][field.dataset[group]];
        if (field.type === 'checkbox') field.checked = value;
        else field.value = value;
      }
    }
    for (const field of root.querySelectorAll('[data-threshold]')) field.value = settings.battery.thresholds[field.dataset.threshold];
    for (const field of root.querySelectorAll('[data-media]')) field.checked = settings.overlay.media[field.dataset.media];
    const previousPreview = preview.value;
    preview.replaceChildren(...settings.previews.map(({ action, label }) => new Option(label, action)));
    if (settings.previews.some(({ action }) => action === previousPreview)) preview.value = previousPreview;
    for (const field of root.querySelectorAll('[name="winx-pos"]')) field.checked = settings.overlay.position === field.value;
    document.getElementById('winx-hotkeys').checked = settings.hotkeysEnabled;
    const talkChord = settings.shortcuts.find(row => row.action === 'talkMode')?.chord;
    document.getElementById('winx-talk-shortcut').textContent = chordName(talkChord);
    document.getElementById('winx-talk-volume').value = settings.talk.volume + '%';
    document.getElementById('winx-talk-fade').value = settings.talk.fadeMs + ' ms';
    $('settings-theme').value = settings.theme;
    $('settings-accent').value = settings.accentColor;
    $('reset-accent').disabled = settings.accentColor === settings.defaultAccent;
    setAccent(settings.accentColor);
    document.documentElement.dataset.theme = settings.theme;
    $('media-diagnostics').checked = settings.mediaDiagnostics;
    talkState(settings.talkStatus);
    renderShortcuts();
  }

  function talkState(state) {
    if (!state) return;
    const test = document.getElementById('winx-test-talk');
    test.disabled = ['activating', 'restoring'].includes(state.phase);
    test.textContent = state.phase === 'active' ? 'Restore settings'
      : 'Activate Talk Mode on device';
    $('winx-talk-status').textContent = state.detail;
    $('recovery-state').textContent = state.earbudRecords || state.outputRecords
      ? 'Saved recovery records · Earbuds ' + state.earbudRecords + ' · Outputs ' + state.outputRecords
      : 'No saved recovery records';
    $('archive-talk').disabled = !state.canArchive || !(state.earbudRecords || state.outputRecords);
    $('recover-talk').disabled = test.disabled || !(state.earbudRecords || state.outputRecords || state.phase === 'active');
  }

  api.onTalkState(talkState);
  document.getElementById('winx-test-talk').addEventListener('click', async () => talkState(await api.talkToggle()));
  $('winx-replay').addEventListener('click', () => api.settingsRoute('replay'));
  $('recover-talk').addEventListener('click', async () => talkState(await api.retryRecovery()));
  $('archive-talk').addEventListener('click', () => { $('archive-confirm').hidden = false; $('archive-no').focus(); });
  $('archive-no').addEventListener('click', () => { $('archive-confirm').hidden = true; $('archive-talk').focus(); });
  $('archive-yes').addEventListener('click', async () => {
    const result = await api.archiveRecovery();
    $('archive-confirm').hidden = true;
    report(result.error);
    talkState(result.status);
  });
  $('settings-startup').addEventListener('change', async event => {
    const result = await api.settingsStartup(event.target.checked);
    event.target.checked = result.startup;
    report(result.error);
  });
  $('settings-accent').addEventListener('change', event => submit('accentColor', event.target.value));
  $('reset-accent').addEventListener('click', () => submit('accentColor', null));
  $('settings-theme').addEventListener('change', event => submit('theme', event.target.value));
  $('media-diagnostics').addEventListener('change', event => submit('mediaDiagnostics', event.target.checked));
  $('export-diagnostics').addEventListener('click', async () => {
    const result = await api.exportDiagnostics();
    $('diagnostics-result').textContent = result.error || 'Saved to ' + result.file;
  });
  for (const button of document.querySelectorAll('[data-route]')) {
    button.addEventListener('click', () => {
      if (button.dataset.route === 'updates') navigate('updates');
      else api.settingsRoute(button.dataset.route);
    });
  }
  document.getElementById('winx-talk-shortcut').addEventListener('click', () => {
    navigate('shortcuts');
    list.closest('details').open = true;
    list.querySelector('[data-shortcut="talkMode"]')?.click();
  });

  function cancelRecording() {
    if (!recording) return;
    recording = null;
    clearTimeout(recordingTimeout);
    submit('recordShortcut', false);
    if (settings) renderShortcuts();
  }

  window.addEventListener('blur', cancelRecording);
  document.addEventListener('visibilitychange', () => { if (document.hidden) cancelRecording(); });
  document.addEventListener('keydown', event => {
    if (!recording) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    if (event.key === 'Escape') { cancelRecording(); return; }
    if (event.repeat || ['Control', 'Shift', 'Alt', 'Meta'].includes(event.key)) return;
    const chord = chordFor(event);
    if (!chord) { report('Use Ctrl or Alt with a letter, number, arrow or function key.'); return; }
    const action = recording;
    recording = null;
    clearTimeout(recordingTimeout);
    submit('shortcut', action, chord);
  }, true);
  root.addEventListener('change', event => {
    if (!settings) return;
    const field = event.target;
    if (field.id === 'winx-hotkeys') submit('hotkeysEnabled', field.checked);
    else if (field.name === 'winx-pos') patch('overlay', 'position', field.value);
    else if (field.dataset.media) patch('media', field.dataset.media, field.checked);
    else if (field.dataset.threshold !== undefined) {
      if (!field.validity.valid || field.value === '') { report('Enter a percentage from 0 to 100.'); return; }
      patch('battery', Number(field.dataset.threshold), Number(field.value));
    } else for (const group of ['overlay', 'battery', 'talk']) {
      const key = field.dataset[group];
      if (key) patch(group, key, field.type === 'checkbox' ? field.checked
        : typeof settings[group][key] === 'number' ? Number(field.value) : field.value);
    }
  });
  document.getElementById('winx-reset-hotkeys').addEventListener('click', () => { cancelRecording(); submit('resetShortcuts'); });
  document.getElementById('winx-show-preview').addEventListener('click', () => submit('preview', preview.value));
  for (const control of root.querySelectorAll('[data-preview]')) {
    control.addEventListener('click', () => submit('preview', control.dataset.preview));
  }
  function deviceChanged(next) {
    const previous = device;
    device = next;
    const connected = next.connected && next.fallback?.mode !== 'unresponsive';
    quick.disabled = !connected;
    for (const button of document.querySelectorAll('[data-device]')) button.disabled = !connected;
    $('settings-device').textContent = next.name || 'No device connected';
    $('settings-device-status').textContent = connected ? 'Connected' : 'Waiting for a compatible Bluetooth device';
    const cap = next.capabilities || {};
    $('settings-capabilities').textContent = [
      cap.transparency && 'Transparency', cap.noiseCancellation && 'Noise cancellation',
      cap.eq?.customId != null && 'Custom EQ', cap.gestures && 'Gestures',
    ].filter(Boolean).join(' · ');
    $('eq-low').textContent = cap.eq?.gainMin == null ? 'Minimum' : cap.eq.gainMin + ' dB';
    $('eq-mid').textContent = cap.eq?.gainMax == null ? 'Maximum' : '+' + cap.eq.gainMax + ' dB';
    $('eq-high').textContent = $('eq-low').textContent;
    if (settings && previous?.name !== next.name) renderShortcuts();
  }
  api.onDeviceState(deviceChanged);
  deviceChanged(init.device);
  settings = await api.awarenessSettings();
  render();
})(window.nx);
