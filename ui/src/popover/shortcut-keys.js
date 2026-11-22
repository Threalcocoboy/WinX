(() => {
  const special = { 32: 'Space', 33: 'Page Up', 34: 'Page Down', 35: 'End', 36: 'Home',
    37: 'Left', 38: 'Up', 39: 'Right', 40: 'Down' };
  const named = { Space: 32, PageUp: 33, PageDown: 34, End: 35, Home: 36,
    ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40 };
  function parts(chord) {
    if (!chord) return [];
    const key = special[chord.vk] || (chord.vk >= 112 ? 'F' + (chord.vk - 111) : String.fromCharCode(chord.vk));
    return [[2, 'Ctrl'], [1, 'Alt'], [4, 'Shift']].filter(([mask]) => chord.modifiers & mask)
      .map(([mask, name]) => ({ name, mask })).concat({ name: key, vk: chord.vk });
  }
  function vkFor(code) {
    if (/^F(?:[1-9]|10|11)$/.test(code)) return 111 + Number(code.slice(1));
    if (/^(?:Key[A-Z]|Digit[0-9])$/.test(code)) return code.charCodeAt(code.length - 1);
    return named[code] || 0;
  }
  const modifiers = event => (event.altKey ? 1 : 0) | (event.ctrlKey ? 2 : 0) | (event.shiftKey ? 4 : 0);
  function chordFor(event) {
    const vk = vkFor(event.code);
    return !event.metaKey && modifiers(event) & 3 && vk ? { modifiers: modifiers(event), vk } : null;
  }
  function name(chord) {
    return parts(chord).map(part => part.name).join(' + ') || 'Unassigned';
  }
  window.winxKeys = { parts, vkFor, modifiers, chordFor, name };
})();
