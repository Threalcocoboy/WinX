window.bindUpdateSettings = function bindUpdateSettings(api, initialState) {
  const $ = id => document.getElementById(id);
  const updateOptions = [...document.querySelectorAll('[data-update-option]')];
  function renderUpdates(state) {
    if (!state?.options) return;
    const consent = $('update-consent');
    const progress = $('update-progress');
    const status = $('update-status');
    $('update-current').textContent = state.current;
    $('update-latest').textContent = state.latest || '—';
    if (status.textContent !== state.status) status.textContent = state.status;
    $('update-checked').textContent = state.checked ? new Date(state.checked * 1000).toLocaleString() : 'Never';
    const error = $('update-error');
    if (error.textContent !== state.error) error.textContent = state.error;
    error.hidden = !state.error;
    const notes = $('update-notes');
    const releaseNotes = state.notes || 'Release notes will appear after a successful check.';
    if (notes.textContent !== releaseNotes) notes.textContent = releaseNotes;
    $('update-progress-group').hidden = !state.downloading;
    progress.max = state.bytes || 1;
    progress.value = state.received || 0;
    $('update-size').textContent = state.bytes ? Math.floor(100 * progress.value / progress.max) + '%' : '';
    $('update-check').disabled = state.busy || state.ready;
    $('update-download').hidden = !state.available || state.ready;
    $('update-download').disabled = state.busy;
    if (state.ready && consent.hidden) {
      consent.hidden = false;
      if (document.querySelector('[data-content="updates"]')?.hidden === false) $('update-cancel').focus();
    } else if (!state.ready) consent.hidden = true;
    for (const input of updateOptions) {
      input.checked = state.options[input.dataset.updateOption];
      input.disabled = input.dataset.updateOption === 'prereleases' && (state.busy || state.ready);
    }
  }
  api.onUpdateState(renderUpdates);
  renderUpdates(initialState);
  for (const input of updateOptions) {
    input.addEventListener('change', async () => renderUpdates(await api.updateSettings(input.dataset.updateOption, input.checked)));
  }
  $('update-check').addEventListener('click', async () => renderUpdates(await api.updateCheck()));
  $('update-download').addEventListener('click', async () => renderUpdates(await api.updateDownload()));
  $('update-confirm').addEventListener('click', async () => renderUpdates(await api.updateConfirm()));
  $('update-cancel').addEventListener('click', async () => {
    renderUpdates(await api.updateCancel());
    $('update-download').focus();
  });
  $('update-release').addEventListener('click', () => api.updateRelease());
};
