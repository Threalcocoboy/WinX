window.createOnboarding = function createOnboarding({ api, symbols, talkStatus, onExit }) {
  const root = document.getElementById('winx-tour');
  const main = document.getElementById('panel');
  const get = id => document.getElementById('tour-' + id);
  const keys = window.winxKeys;
  const audio = get('audio');
  const motion = window.matchMedia('(prefers-reduced-motion: reduce)');
  const pages = [...root.querySelectorAll('[data-scene]')];
  const featured = ['panel', 'cycleNoise', 'transparency', 'lowLatency', 'talkMode'];
  let page = 0;
  let prefs;
  let state = talkStatus || { phase: 'off' };
  let devices;
  let recording;
  let recordTimer;
  let introTimer;
  let connectionTimer;
  let searchTimer;
  let pressTimer;
  let finishTimer;
  let gainFrame;
  let serial = Promise.resolve();
  let demo = 'idle';
  let previewActive = false;
  let demoComplete = false;
  let triedActive = false;
  let activationComplete = false;
  let waitingFor = null;
  let deviceListStamp;
  let demoError;
  const down = new Set();
  let heldModifiers = 0;

  function report(message = '') { get('error').textContent = message; }
  function shortcut() { return prefs?.shortcuts.find(entry => entry.action === 'talkMode'); }

  function showPage(index) {
    if (page === 4 && page !== index) stopDemo();
    clearTimeout(introTimer);
    clearTimeout(connectionTimer);
    clearTimeout(finishTimer);
    page = index;
    for (const [i, section] of pages.entries()) {
      section.hidden = i !== index;
      const marker = root.querySelector('[data-step-dot="' + i + '"]');
      marker.toggleAttribute('data-done', i < index);
      if (i === index) marker.setAttribute('aria-current', 'step');
      else marker.removeAttribute('aria-current');
    }
    get('footer').hidden = page === 0 || page === 5;
    get('next').hidden = page === 1;
    get('next').disabled = false;
    get('next').textContent = 'Continue';
    root.querySelector('.tour-scenes').scrollTop = 0;
    report();
    pages[index].querySelector('h1').focus({ preventScroll: true });
    if (index === 0 && !motion.matches) introTimer = setTimeout(() => {
      if (!root.hidden && page === 0 && document.hasFocus()) showPage(1);
    }, 3900);
    if (index === 1) { drawDevices(); scan(); }
    if (index === 2) drawNotifications();
    if (index === 3) drawShortcuts();
    if (index === 4) { renderKeys(); drawDemo(); }
  }

  async function open() {
    if (!root.hidden) return;
    prefs = await api.awarenessSettings();
    state = prefs.talkStatus || state;
    root.hidden = false;
    main.inert = true;
    document.documentElement.dataset.touring = '';
    waitingFor = null;
    demoComplete = false;
    showPage(0);
    const result = await api.onboarding('open');
    if (result.error) close();
  }

  function close() {
    clearTimeout(introTimer);
    clearTimeout(connectionTimer);
    clearTimeout(searchTimer);
    clearTimeout(finishTimer);
    cancelRecord(false);
    stopDemo();
    root.hidden = true;
    main.inert = false;
    delete document.documentElement.dataset.touring;
    onExit();
  }

  function command(...args) {
    serial = serial.then(async () => {
      const result = await api.setAwarenessSettings(...args);
      prefs = result.settings;
      if (page === 2) drawNotifications();
      if (page === 3) drawShortcuts();
      report(result.error);
      return !result.error;
    }).catch(() => { report('WinX could not save that setting.'); return false; });
    return serial;
  }

  function overlayPatch(field, value) {
    serial = serial.then(async () => {
      const next = { ...prefs.overlay };
      if (field === 'connection') Object.assign(next, { connected: value, disconnected: value });
      else if (field === 'media') next.media = { playPause: value, track: value, volume: value };
      else next[field] = value;
      const group = field === 'battery' ? 'battery' : 'overlay';
      const result = await api.setAwarenessSettings(group, group === 'battery' ? { ...prefs.battery, enabled: value } : next);
      prefs = result.settings;
      drawNotifications();
      report(result.error);
      return !result.error;
    }).catch(() => { report('WinX could not save your notification choices.'); return false; });
  }

  function drawNotifications() {
    for (const input of pages[2].querySelectorAll('input')) {
      if (input.name === 'tour-position') input.checked = prefs.overlay.position === input.value;
      else {
        const name = input.dataset.notice;
        const values = name === 'connection' ? [prefs.overlay.connected, prefs.overlay.disconnected]
          : name === 'media' ? Object.values(prefs.overlay.media)
            : [name === 'battery' ? prefs.battery.enabled : prefs.overlay[name]];
        input.checked = values.every(Boolean);
        input.indeterminate = values.some(Boolean) && !input.checked;
      }
    }
  }

  function createButton(label, action, description = label) {
    const button = document.createElement('button');
    button.className = 'winx-button';
    button.type = 'button';
    button.textContent = label;
    if (description !== label) button.setAttribute('aria-label', description);
    button.addEventListener('click', action);
    return button;
  }

  function useSymbol(icon, name) {
    icon.style.setProperty('--sym', 'url("' + (symbols[name] || symbols.budLeft) + '")');
  }

  function scan() {
    clearTimeout(searchTimer);
    get('search-state').firstElementChild.hidden = false;
    get('search-state').lastElementChild.textContent = 'Searching for paired devicesâ€¦';
    api.onboarding('rescan');
    searchTimer = setTimeout(() => {
      if (page === 1) drawDevices(true);
    }, 4500);
  }

  function drawDevices(settled = false) {
    const list = get('devices');
    const ready = devices?.ready && (!waitingFor || devices.id === waitingFor);
    get('connected').hidden = !ready;
    get('keep').disabled = !ready;
    get('search-state').hidden = Boolean(ready && (!waitingFor || devices.id === waitingFor));
    useSymbol(get('device-mark'), devices?.name.toLowerCase().includes('headphone') ? 'headphones' : 'budLeft');
    if (ready) {
      get('device-name').textContent = devices.name;
      get('device-battery').textContent = devices.battery;
    }
    const stamp = JSON.stringify(devices?.devices);
    if (stamp !== deviceListStamp || list.childElementCount === 0) {
      deviceListStamp = stamp;
      list.replaceChildren(...(devices?.devices || []).map(item => {
        const row = document.createElement('li');
        const button = createButton(item.name, async () => {
          waitingFor = item.id;
          clearTimeout(connectionTimer);
          const result = await api.onboarding('select', item.id);
          if (result.error) {
            waitingFor = null;
            get('search-state').hidden = false;
            get('search-state').lastElementChild.textContent = result.error;
          } else {
            drawDevices();
            clearTimeout(searchTimer);
            searchTimer = setTimeout(() => { if (page === 1) drawDevices(true); }, 7000);
          }
        }, 'Select ' + item.name);
        const icon = document.createElement('i');
        icon.className = 'sym';
        icon.setAttribute('aria-hidden', 'true');
        useSymbol(icon, item.name.toLowerCase().includes('headphone') ? 'headphones' : 'budLeft');
        const status = document.createElement('small');
        status.textContent = item.linked ? 'Linked in Windows' : 'Paired';
        button.prepend(icon);
        button.append(status);
        row.append(button);
        return row;
      }));
    }
    const search = get('search-state');
    if (!search.hidden) {
      search.firstElementChild.hidden = settled;
      search.lastElementChild.textContent = waitingFor
        ? settled ? 'WinX could not confirm a connection. Connect the earbuds in Windows and search again.' : 'Connecting to your deviceâ€¦'
        : settled ? devices?.devices.length
          ? 'Select a device. If it is paired but offline, connect it in Windows first.'
          : 'No compatible devices found. Put your earbuds in pairing mode and open Bluetooth Settings.'
        : 'Looking for a compatible deviceâ€¦';
    }
    if (ready && devices.id === waitingFor && !connectionTimer) {
      connectionTimer = setTimeout(() => {
        connectionTimer = null;
        if (page === 1 && devices?.ready && devices.id === waitingFor) showPage(2);
      }, motion.matches ? 140 : 1000);
    }
  }

  function cancelRecord(repaint = true) {
    if (!recording) return;
    recording = null;
    clearTimeout(recordTimer);
    command('recordShortcut', false);
    if (repaint && page === 3) drawShortcuts();
  }

  function drawShortcuts() {
    const list = get('shortcuts');
    const focus = document.activeElement?.dataset.tourShortcut;
    get('hotkeys').checked = prefs.hotkeysEnabled;
    get('recording').textContent = recording ? 'Press the keys together. Esc cancels.' : '';
    list.replaceChildren(...featured.map(action => {
      const entry = prefs.shortcuts.find(item => item.action === action);
      const row = document.createElement('div');
      row.className = 'tour-shortcut';
      row.toggleAttribute('data-feature', action === 'talkMode');
      const name = document.createElement('span');
      name.textContent = entry.label;
      const chord = document.createElement('small');
      chord.textContent = keys.name(entry.chord) + (entry.chord && prefs.hotkeysEnabled && !entry.available && !recording ? ' Â· Unavailable in Windows' : '');
      name.append(chord);
      const edit = createButton(recording === action ? 'Cancel' : 'Edit', () => {
        if (recording === action) { cancelRecord(); return; }
        recording = action;
        clearTimeout(recordTimer);
        recordTimer = setTimeout(cancelRecord, 19000);
        command('recordShortcut', true);
        drawShortcuts();
      }, 'Edit ' + entry.label);
      edit.dataset.tourShortcut = action;
      const clear = createButton('Clear', () => {
        cancelRecord(false);
        command('shortcut', action, null);
      }, 'Clear ' + entry.label);
      clear.disabled = !entry.chord;
      row.append(name, edit, clear);
      return row;
    }));
    if (focus) list.querySelector('[data-tour-shortcut="' + focus + '"]')?.focus();
  }

  function renderKeys() {
    const host = get('demo-keys');
    host.replaceChildren(...keys.parts(shortcut()?.chord).map(part => {
      const cap = document.createElement('kbd');
      cap.textContent = part.name;
      if (part.mask) cap.dataset.mask = part.mask;
      if (part.vk) cap.dataset.vk = part.vk;
      return cap;
    }));
  }

  function paintKeys() {
    const host = get('demo-keys');
    if (host.dataset.confirmed !== undefined) return;
    for (const cap of host.children) {
      const pressed = cap.dataset.mask ? heldModifiers & Number(cap.dataset.mask) : down.has(Number(cap.dataset.vk));
      cap.toggleAttribute('data-pressed', Boolean(pressed));
    }
  }

  function confirmKeys() {
    clearTimeout(pressTimer);
    const host = get('demo-keys');
    host.dataset.confirmed = '';
    for (const cap of host.children) cap.setAttribute('data-pressed', '');
    pressTimer = setTimeout(() => {
      delete host.dataset.confirmed;
      paintKeys();
    }, 260);
  }

  function clearKeys() {
    clearTimeout(pressTimer);
    delete get('demo-keys').dataset.confirmed;
    down.clear();
    heldModifiers = 0;
    for (const cap of get('demo-keys').children) cap.removeAttribute('data-pressed');
  }

  function startDemo(preview) {
    if (demo === 'real' && state.phase !== 'off') return;
    demo = preview ? 'preview' : 'real';
    demoError = '';
    previewActive = false;
    demoComplete = false;
    triedActive = false;
    activationComplete = false;
    audio.volume = 0.45;
    if (!preview) playDemoAudio();
    api.onboarding('demo', !preview);
    drawDemo();
  }

  function playDemoAudio() {
    audio.play().catch(() => {
      demoError = 'The local audio could not start. You can still try the shortcut.';
      drawDemo();
    });
  }

  function toggleDemo() {
    if (demo === 'preview') {
      previewActive = !previewActive;
      demoComplete = !previewActive;
      if (!audio.paused) fadePreview(previewActive ? 0.08 : 0.45);
      drawDemo();
      return;
    }
    if (demo === 'real') api.talkToggle().then(updateTalk);
  }

  function fadePreview(target) {
    cancelAnimationFrame(gainFrame);
    const started = performance.now();
    const from = audio.volume;
    const duration = prefs.talk.fadeMs;
    function frame(now) {
      const progress = Math.min(1, (now - started) / duration);
      audio.volume = from + (target - from) * (progress * progress * (3 - 2 * progress));
      if (progress < 1) gainFrame = requestAnimationFrame(frame);
    }
    gainFrame = requestAnimationFrame(frame);
  }

  function stopDemo() {
    audio.pause();
    audio.currentTime = 0;
    cancelAnimationFrame(gainFrame);
    if (demo !== 'idle') api.onboarding('demo', false);
    demo = 'idle';
    previewActive = false;
    clearKeys();
  }

  function drawDemo() {
    const physical = demo === 'real';
    const preview = demo === 'preview';
    const active = physical ? state.phase === 'active' : previewActive;
    const busy = physical && ['activating', 'restoring'].includes(state.phase);
    const next = get('next');
    const title = get('talk-title');
    const subtitle = get('demo-subtitle');
    const status = get('demo-status');
    const cap = shortcut();
    pages[4].toggleAttribute('data-talking', active);
    pages[4].toggleAttribute('data-playing', !audio.paused && !audio.muted);
    const scale = physical
      ? state.volume === null ? 1 : Math.max(.16, Math.min(1, state.volume / 25))
      : .30;
    pages[4].style.setProperty('--demo-wave-scale', scale);
    title.textContent = active ? (preview ? 'Talk Mode preview' : 'Talk Mode is On')
      : demoComplete && !preview && activationComplete ? 'Back to your music.'
        : demoComplete ? 'Now you know the shortcut.' : 'Meet Talk Mode.';
    subtitle.textContent = active ? (preview ? 'Simulated transparency and lower volume.' : 'The world is back in focus.')
      : demoComplete && !preview && activationComplete ? 'Exactly how you left it.'
        : 'Hear someone speaking without removing your earbuds.';
    get('demo-start').hidden = physical || (preview && !audio.paused);
    get('demo-start').textContent = preview ? 'Play preview audio' : 'Start demo audio';
    get('demo-start').disabled = !preview && (!devices?.ready || !prefs.talk.enabled || state.phase !== 'off');
    get('demo-toggle').hidden = demo === 'idle';
    get('demo-toggle').disabled = busy || (physical && state.phase === 'recovery');
    get('demo-toggle').textContent = active ? 'Restore' : preview ? 'Simulate Talk Mode' : 'Toggle Talk Mode';
    get('preview-demo').hidden = preview;
    get('preview-demo').disabled = physical && state.phase !== 'off';
    get('mute').setAttribute('aria-pressed', String(audio.muted));
    get('mute').textContent = audio.muted ? 'Unmute demo' : 'Mute demo';
    const instruction = cap?.chord ? 'Press the shortcut' : 'Select the demo button';
    if (demoError) status.textContent = demoError;
    else if (preview) status.textContent = active ? 'Preview only. ' + instruction + ' again to return.' : (demoComplete ? 'Preview finished. No earbud or Windows volume settings were changed.' : 'Preview only. ' + instruction + ' to see the effect.');
    else if (physical) status.textContent = busy || state.phase === 'recovery' ? state.detail
      : active ? state.detail + ' Â· ' + instruction + ' again to restore.'
        : demoComplete ? state.detail : 'Imagine someone starts talking to you. ' + instruction + ' when youâ€™re ready.';
    else status.textContent = devices?.ready && prefs.talk.enabled ? 'Start audio to hear the change.' : 'A connected compatible device and enabled Talk Mode are required for the live demo.';
    next.disabled = busy || (physical && state.phase !== 'off');
    next.textContent = demoComplete ? 'Continue' : 'Skip demo';
  }

  function updateTalk(next) {
    const previous = state.phase;
    state = next;
    if (root.hidden || page !== 4 || demo !== 'real') return;
    if (state.phase === 'active') {
      triedActive = true;
      activationComplete = state.complete;
    }
    if (previous !== 'off' && state.phase === 'off' && triedActive) {
      demoComplete = true;
      clearTimeout(finishTimer);
      if (activationComplete && state.complete && !motion.matches) {
        finishTimer = setTimeout(() => { if (page === 4 && document.hasFocus()) showPage(5); }, 2200);
      }
    } else if (state.phase === 'off' && state.detail && !state.complete) demoError = state.detail;
    drawDemo();
  }

  root.addEventListener('change', event => {
    if (event.target.name === 'tour-position') overlayPatch('position', event.target.value);
    else if (event.target.dataset.notice) overlayPatch(event.target.dataset.notice, event.target.checked);
    else if (event.target === get('hotkeys')) command('hotkeysEnabled', event.target.checked);
  });

  root.addEventListener('click', async event => {
    const action = event.target.closest('[data-tour]')?.dataset.tour;
    switch (action) {
      case 'next':
        if ((await serial) === false) break;
        cancelRecord(false);
        showPage(Math.min(5, page + 1));
        break;
      case 'back': cancelRecord(false); showPage(Math.max(0, page - 1)); break;
      case 'rescan': scan(); break;
      case 'pair': api.onboarding('pair'); break;
      case 'automatic': await api.onboarding('select', 0); waitingFor = null; scan(); break;
      case 'keepDevice': {
        const result = await api.onboarding('select', devices.id);
        if (!result.error) showPage(2);
        else get('search-state').lastElementChild.textContent = result.error;
        break;
      }
      case 'skipDevice': showPage(2); break;
      case 'overlayPreview': command('preview', 'ancOn'); break;
      case 'resetShortcuts': cancelRecord(false); command('resetShortcuts'); break;
      case 'demoStart': if (demo === 'preview') playDemoAudio(); else startDemo(false); break;
      case 'previewDemo': startDemo(true); break;
      case 'demoToggle': toggleDemo(); break;
      case 'mute': audio.muted = !audio.muted; drawDemo(); break;
      case 'finish':
      case 'leave': {
        await serial;
        const result = await api.onboarding(action);
        if (result.error) {
          get('footer').hidden = false;
          get('next').hidden = true;
          report(result.error);
        } else close();
        break;
      }
    }
  });

  document.addEventListener('keydown', event => {
    if (root.hidden) return;
    if (recording && page === 3) {
      event.preventDefault();
      event.stopImmediatePropagation();
      if (event.key === 'Escape') { cancelRecord(); return; }
      const held = [[event.ctrlKey, 'Ctrl'], [event.altKey, 'Alt'], [event.shiftKey, 'Shift']]
        .filter(([pressed]) => pressed).map(([, name]) => name);
      if (held.length) get('recording').textContent = held.join(' + ') + ' + â€¦';
      if (event.repeat || ['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) return;
      const chord = keys.chordFor(event);
      if (!chord) { report('Use Ctrl or Alt with a supported key.'); return; }
      const action = recording;
      recording = null;
      clearTimeout(recordTimer);
      command('shortcut', action, chord);
      return;
    }
    if (page !== 4 || demo === 'idle') return;
    const entry = shortcut();
    const chord = entry?.chord;
    if (!chord) return;
    const vk = keys.vkFor(event.code);
    if (vk === chord.vk) down.add(vk);
    heldModifiers = keys.modifiers(event);
    paintKeys();
    if (!event.repeat && vk === chord.vk && keys.modifiers(event) === chord.modifiers && !event.metaKey
      && (!prefs.hotkeysEnabled || !entry.available)) {
      event.preventDefault();
      confirmKeys();
      toggleDemo();
    }
  }, true);
  document.addEventListener('keyup', event => {
    if (root.hidden || page !== 4) return;
    down.delete(keys.vkFor(event.code));
    heldModifiers = keys.modifiers(event);
    paintKeys();
  });
  window.addEventListener('blur', () => { cancelRecord(); clearKeys(); });
  document.addEventListener('visibilitychange', () => {
    if (!root.hidden && document.hidden) { cancelRecord(); stopDemo(); if (page === 4) drawDemo(); }
  });
  api.onTourShortcut(() => {
    if (root.hidden || page !== 4 || demo === 'idle') return;
    confirmKeys();
    if (demo === 'preview') toggleDemo();
  });
  api.onTourDevices(next => {
    devices = next;
    if (root.hidden) return;
    if (page === 1) drawDevices();
    if (page === 4) drawDemo();
  });
  api.onTalkState(updateTalk);
  audio.addEventListener('playing', () => { if (page === 4) drawDemo(); });
  audio.addEventListener('pause', () => { if (page === 4) drawDemo(); });
  return { open };
};
