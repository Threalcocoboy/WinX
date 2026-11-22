// Compatibility bridge only. The original renderer, markup and styles are unchanged.
(() => {
  let sequence = 0;
  const replies = new Map();
  const listeners = { state: new Set(), accent: new Set(), talk: new Set(), tourDevices: new Set(), tourShortcut: new Set(), route: new Set(), updates: new Set() };
  function send(command, args = [], reply = false) {
    const id = ++sequence;
    if (!reply) { window.ipc.postMessage(JSON.stringify({ id, command, args })); return; }
    return new Promise(resolve => {
      replies.set(id, resolve);
      window.ipc.postMessage(JSON.stringify({ id, command, args }));
    });
  }
  window.__nxReply = (id, value) => { const resolve = replies.get(id); replies.delete(id); resolve?.(value); };
  window.__nxState = state => { for (const listener of listeners.state) listener(state); };
  window.__nxAccent = value => { for (const listener of listeners.accent) listener(value); };
  window.__nxTalkState = value => { for (const listener of listeners.talk) listener(value); };
  window.__nxTourDevices = value => { for (const listener of listeners.tourDevices) listener(value); };
  window.__nxTourShortcut = () => { for (const listener of listeners.tourShortcut) listener(); };
  window.__nxRoute = route => { for (const listener of listeners.route) listener(route); };
  window.__nxUpdateState = state => { for (const listener of listeners.updates) listener(state); };
  window.nx = {
    openSettings: () => send('openSettings'),
    onRoute: listener => listeners.route.add(listener),
    settingsRoute: route => send('settingsRoute', [route]),
    settingsStartup: enabled => send('settingsStartup', [enabled], true),
    retryRecovery: () => send('retryRecovery', [], true),
    archiveRecovery: () => send('archiveRecovery', [true], true),
    exportDiagnostics: () => send('exportDiagnostics', [], true),
    talkToggle: () => send('talkToggle', [], true),
    talkStop: () => send('talkStop'),
    onTalkState: listener => listeners.talk.add(listener),
    onTourDevices: listener => listeners.tourDevices.add(listener),
    onTourShortcut: listener => listeners.tourShortcut.add(listener),
    onboarding: (...args) => send('onboarding', args, true),
    init: () => send('init', [], true),
    setHeight: height => send('setHeight', [height]),
    onAccent: listener => listeners.accent.add(listener),
    onDeviceState: listener => listeners.state.add(listener),
    quit: () => send('quit'),
    openExternal: target => send('openExternal', [target]),
    updateAction: () => send('updateAction'),
    updateSettings: (key, value) => send('updateSettings', [key, value], true),
    updateCheck: () => send('updateCheck', [], true),
    updateDownload: () => send('updateDownload', [], true),
    updateConfirm: () => send('updateConfirm', [true], true),
    updateCancel: () => send('updateCancel', [], true),
    updateRelease: () => send('updateRelease'),
    onUpdateState: listener => listeners.updates.add(listener),
    awarenessSettings: () => send('awarenessSettings', [], true),
    setAwarenessSettings: (...args) => send('setAwarenessSettings', args, true),
    popupMenu: (items, x, y) => send('popupMenu', [items, x, y], true),
  };
  for (const command of ['setNoiseMode', 'setAncStrength', 'setBass', 'setQuickToggle', 'ring', 'setSpatialAudio', 'setDualConnection', 'setDualDevice', 'setGesture', 'setGestureNoiseCycle', 'setEqPreset', 'setCustomEq']) {
    window.nx[command] = (...args) => send(command, args);
  }
  window.addEventListener('error', event => send('uiError', [event.message]));
  window.addEventListener('unhandledrejection', event => send('uiError', [String(event.reason)]));
  window.addEventListener('blur', () => send('windowBlur'));
  window.addEventListener('contextmenu', event => event.preventDefault());
  const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
  send('motionPreference', [reducedMotion.matches]);
  reducedMotion.addEventListener('change', event => send('motionPreference', [event.matches]));
})();
