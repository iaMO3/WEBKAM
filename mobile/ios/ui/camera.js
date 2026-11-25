const byId = (id) => document.getElementById(id);
const chip = byId('connection-chip');
const form = byId('pair');
const pin = byId('pin');
const section = byId('camera');
const stage = byId('phone-stage');
const picker = byId('camera-picker');
const toggle = byId('toggle');
const floating = byId('float-camera');
const flash = byId('camera-flash');
const flashNote = byId('flash-note');
const mode = byId('native-mode');
const send = (action, fields = {}) => window.webkit?.messageHandlers.pocketcam.postMessage({ action, ...fields });
let reportedFrame;
let layoutPending = false;
let lenses = [];

function sendLayout() {
  if (layoutPending) return;
  layoutPending = true;
  requestAnimationFrame(() => {
    layoutPending = false;
    const frame = stage.getBoundingClientRect();
    const viewport = window.visualViewport;
    const geometry = {
      x: frame.left - (viewport?.offsetLeft ?? 0),
      y: frame.top - (viewport?.offsetTop ?? 0),
      width: frame.width,
      height: frame.height,
      viewportWidth: viewport?.width ?? window.innerWidth
    };
    const key = JSON.stringify(geometry);
    if (reportedFrame === key) return;
    reportedFrame = key;
    send('layout', geometry);
  });
}

window.renderPocketCam = (state) => {
  const phase = state.phase;
  chip.dataset.state = phase;
  chip.textContent = { live: 'Live', ready: 'Ready', pairing: 'Pairing', pending: 'Connecting', offline: 'Offline' }[phase];
  byId('phone-status').textContent = state.message;
  form.hidden = state.paired || state.checking || state.retry;
  pin.disabled = state.pairing;
  byId('pair-button').disabled = state.pairing;
  byId('retry').hidden = !state.retry;
  section.hidden = !state.paired;
  byId('camera-idle').hidden = state.preview;
  byId('camera-idle-text').textContent = state.running ? 'Opening camera…' : 'Camera is off';
  stage.dataset.orientation = state.portrait ? 'portrait' : 'landscape';
  if (JSON.stringify(lenses) !== JSON.stringify(state.lenses)) {
    lenses = state.lenses;
    picker.replaceChildren(...lenses.map(({name, id}) => new Option(name, id)));
  }
  picker.value = state.lens;
  picker.disabled = !lenses.length || state.opening || state.pipStarting || state.flashBusy;
  const activeLens = lenses.find(lens => lens.id === state.lens);
  for (const button of document.querySelectorAll('[data-facing]')) {
    button.disabled = picker.disabled || !lenses.some(lens => lens.facing === button.dataset.facing);
    button.setAttribute('aria-pressed', String(activeLens?.facing === button.dataset.facing));
  }
  for (const button of document.querySelectorAll('[data-zoom]')) {
    const factor = Number(button.dataset.zoom);
    button.disabled = !state.preview || factor > state.maxZoom;
    button.setAttribute('aria-pressed', String(Math.abs(factor - state.zoom) < 0.01));
  }
  byId('light-label').textContent = state.screenLight ? 'Screen light' : 'Flash';
  byId('light-state').textContent = state.flashOn ? 'On' : 'Off';
  document.documentElement.classList.toggle('screen-light', !!(state.screenLight && state.flashOn));
  flash.disabled = state.flashBusy || !state.flashAvailable;
  flash.setAttribute('aria-pressed', String(state.flashOn));
  flashNote.textContent = state.flashNote;
  flashNote.hidden = !state.flashNote;
  toggle.textContent = state.running ? 'Stop camera' : 'Start camera';
  toggle.classList.toggle('stop-camera', state.running);
  toggle.disabled = !state.lens;
  floating.hidden = !state.pip;
  floating.disabled = state.pipStarting;
  floating.textContent = state.floating ? 'Close floating preview' : 'Floating preview';
  mode.textContent = state.mode;
  mode.hidden = !state.mode || !state.running;
  sendLayout();
};

form.addEventListener('submit', (event) => {
  event.preventDefault();
  pin.blur();
  send('pair', {code: pin.value});
});
byId('retry').addEventListener('click', () => send('retry'));
picker.addEventListener('change', () => send('lens', {id: picker.value}));
for (const button of document.querySelectorAll('[data-facing]')) {
  button.addEventListener('click', () => {
    if (button.getAttribute('aria-pressed') === 'true') return;
    const choices = lenses.filter(lens => lens.facing === button.dataset.facing);
    const lens = choices.find(choice => choice.standard) || choices[0];
    if (lens) send('lens', {id: lens.id});
  });
}
for (const button of document.querySelectorAll('[data-zoom]')) {
  button.addEventListener('click', () => send('zoom', {factor: Number(button.dataset.zoom)}));
}
toggle.addEventListener('click', () => send('camera'));
flash.addEventListener('click', () => send('flash'));
floating.addEventListener('click', () => {
  stage.scrollIntoView({block: 'nearest'});
  sendLayout();
  requestAnimationFrame(() => send('pip'));
});
window.addEventListener('scroll', sendLayout, {passive: true});
window.addEventListener('resize', sendLayout);
window.visualViewport?.addEventListener('resize', sendLayout);
window.visualViewport?.addEventListener('scroll', sendLayout);
new ResizeObserver(sendLayout).observe(stage);
send('ready');
sendLayout();
