const byId = (id) => document.getElementById(id);
const video = byId('video');
const form = byId('pair');
const panel = byId('camera');
const toggle = byId('toggle');
const floatCamera = byId('float-camera');
const flash = byId('camera-flash');
const flashNote = byId('flash-note');
const JPEG_QUALITY = 0.88;
const LARGE_FRAME_QUALITY = 0.80;
const MAX_JPEG_BYTES = 1500000;
const floatNote = byId('float-note');
const cameraHint = byId('camera-hint');
const picker = byId('camera-picker');
const status = byId('phone-status');
const connectionChip = byId('connection-chip');
const modeLabel = byId('mode');
const canvas = byId('output');
const context = canvas.getContext('2d', { alpha: false });
let mode = { width: 1920, height: 1080, fps: 30 };
let media;
let socket;
let streamId;
let wakeLock;
let generation = 0;
let busy = false;
let landscapeAngle = 90;
let nativePip = false;
let pendingPip = 0;
let browserPipRejected = false;
let flashOn = false;
let flashBusy = false;
let cameraSettings = Promise.resolve();

function message(value, state = panel.hidden ? 'pairing' : 'ready') {
  status.textContent = value;
  connectionChip.dataset.state = state;
  connectionChip.textContent = {
    pairing: 'Pairing', ready: 'Ready', pending: 'Connecting', live: 'Live', offline: 'Offline'
  }[state];
}
function request(path, options) { return fetch('/api/' + path, options); }

const ios = /iPhone|iPad|iPod/.test(navigator.userAgent) ||
  (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);
const androidPip = /\bPocketCamAndroid\/PiP\b/.test(navigator.userAgent);
const standalone = window.matchMedia('(display-mode: standalone)').matches || navigator.standalone;
if (location.protocol === 'https:' && ios && !standalone && /Version\/.+Safari\//.test(navigator.userAgent)) {
  byId('home-screen-install').hidden = false;
}
if (window.isSecureContext && 'serviceWorker' in navigator) {
  navigator.serviceWorker.register('/worker.js').catch(() => {});
}

function showPairing() {
  panel.hidden = true;
  form.hidden = false;
  message('Enter the eight-digit code shown in WEBKAM on your PC.');
}

function showCamera() {
  form.hidden = true;
  panel.hidden = false;
  message('Paired with your PC. Start your camera when ready.');
  listCameras();
}

function constraints(selected) {
  return {
    width: { ideal: Math.max(selected.width, selected.height) },
    height: { ideal: Math.min(selected.width, selected.height) },
    frameRate: { ideal: selected.fps, max: selected.fps }
  };
}

function supportsFlash(track) {
  const torch = track?.getCapabilities?.().torch;
  return torch === true || (Array.isArray(torch) && torch.includes(true));
}

function applyCameraSettings(track, requestedFlash) {
  // applyConstraints replaces the previous settings. Keep the selected resolution
  // and torch together, and order changes from the PC and phone.
  const apply = cameraSettings.then(() => {
    if (track !== media?.getVideoTracks()[0]) return;
    const settings = constraints(mode);
    if (supportsFlash(track)) settings.advanced = [{ torch: requestedFlash ?? flashOn }];
    return track.applyConstraints(settings);
  });
  cameraSettings = apply.catch(() => {});
  return apply;
}

async function toggleFlash() {
  const track = media?.getVideoTracks()[0];
  if (!supportsFlash(track) || busy || flashBusy) return;
  const next = !flashOn;
  flashBusy = true;
  flashNote.hidden = true;
  updateButtons();
  try {
    await applyCameraSettings(track, next);
    if (track === media?.getVideoTracks()[0]) flashOn = next;
  } catch {
    if (track === media?.getVideoTracks()[0]) {
      flashNote.textContent = 'The flash couldn’t switch. Try again or choose a different rear lens.';
      flashNote.hidden = false;
    }
  } finally {
    if (track === media?.getVideoTracks()[0]) {
      flashBusy = false;
      updateButtons();
    }
  }
}

async function listCameras() {
  try {
    const devices = await navigator.mediaDevices.enumerateDevices();
    const selected = picker.value;
    const cameras = devices.filter(device => device.kind === 'videoinput' && device.deviceId);
    const options = cameras.map((camera, index) =>
      new Option(camera.label || `Camera ${index + 1}`, 'id:' + camera.deviceId));
    while (picker.options.length > 2) picker.remove(2);
    picker.append(...options);
    picker.value = [...picker.options].some(option => option.value === selected) ? selected : 'environment';
  } catch {
    // The automatic front and rear choices work before camera permission is granted.
  }
}

function setMode(next) {
  if (![720, 1080, 1280, 1920].includes(next.width) ||
      ![720, 1080, 1280, 1920].includes(next.height) ||
      ![30, 60].includes(next.fps)) return;
  const cameraModeChanged = next.fps !== mode.fps || Math.max(next.width, next.height) !== Math.max(mode.width, mode.height);
  mode = next;
  if (cameraModeChanged && media) applyCameraSettings(media.getVideoTracks()[0]).catch(() => {});
  if (media && !canvas.hidden) updateOutput(canvas.width, canvas.height);
  else updateOutput(mode.width, mode.height);
}

function updateOutput(width, height) {
  if (canvas.width !== width) canvas.width = width;
  if (canvas.height !== height) canvas.height = height;
  byId('phone-stage').dataset.orientation = width > height ? 'landscape' : 'portrait';
  modeLabel.textContent = width + ' × ' + height + ' · ' + mode.fps + ' FPS target';
}

async function syncPairing() {
  try {
    const response = await request('phone');
    if (response.ok) {
      setMode(await response.json());
      showCamera();
    } else {
      showPairing();
    }
  } catch {
    message('Could not reach WEBKAM. Check that it’s open on your PC.', 'offline');
  }
}

if (!navigator.mediaDevices?.getUserMedia) {
  message('This page needs a trusted connection. Complete the certificate setup or use the Android ADB connection.');
} else {
  syncPairing();
}

form.addEventListener('submit', async (event) => {
  event.preventDefault();
  const code = byId('pin').value.replace(/\s/g, '');
  if (!/^\d{8}$/.test(code)) { message('Enter all eight digits from your PC.'); return; }
  const button = form.querySelector('button');
  button.disabled = true;
  try {
    const response = await request('pair', {
      method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ code })
    });
    if (response.ok) await syncPairing();
    else if (response.status === 409) message('Another browser is paired. Select Disconnect on the PC and try again.');
    else if (response.status === 429) message('Too many attempts. Wait 30 seconds and try again.');
    else message('That code didn’t match. Check the code on your PC.');
  } catch {
    message('Could not reach WEBKAM. Check your connection and try again.');
  } finally {
    button.disabled = false;
  }
});

function updateButtons() {
  toggle.disabled = busy;
  picker.disabled = busy;
  flash.hidden = !supportsFlash(media?.getVideoTracks()[0]);
  flash.disabled = busy || flashBusy;
  flash.setAttribute('aria-pressed', String(flashOn));
  toggle.textContent = media ? 'Stop camera' : 'Start camera';
  toggle.classList.toggle('stop-camera', !!media);
  const available = !!media && (androidPip || (!browserPipRejected && video.readyState >= 2 && browserPipSupported()));
  floatCamera.hidden = !available || nativePip;
  floatCamera.disabled = busy || !!pendingPip;
  floatCamera.textContent = isFloating() ? 'Close floating preview' : 'Floating preview';
  cameraHint.hidden = !floatNote.hidden;
  cameraHint.textContent = available
    ? 'Floating preview depends on support for live camera video on this device.'
    : 'Keep WEBKAM open while streaming.';
}

function webKitPipSupported() {
  return typeof video.webkitSetPresentationMode === 'function' &&
    video.webkitSupportsPresentationMode?.('picture-in-picture');
}

function browserPipSupported() {
  return (document.pictureInPictureEnabled && typeof video.requestPictureInPicture === 'function') ||
    webKitPipSupported();
}

function isFloating() {
  return nativePip || document.pictureInPictureElement === video ||
    video.webkitPresentationMode === 'picture-in-picture';
}

function finishPipRequest() {
  clearTimeout(pendingPip);
  pendingPip = 0;
  updateButtons();
}

function pauseIfHidden() {
  if (document.hidden && (media || busy) && !pendingPip && !isFloating()) {
    stopCamera();
    message('Camera paused. Tap Start camera to reconnect.');
  }
}

function floatingFailed() {
  if (ios) browserPipRejected = true;
  floatNote.textContent = ios
    ? 'This iPhone paused or rejected the floating live camera. Keep WEBKAM open while streaming.'
    : 'Floating preview is unavailable. Keep WEBKAM open while streaming.';
  floatNote.hidden = false;
  finishPipRequest();
  pauseIfHidden();
}

async function toggleFloatingPreview() {
  if (!media || busy || pendingPip) return;
  floatNote.hidden = true;
  const opening = !isFloating();
  pendingPip = setTimeout(() => {
    if (opening && !isFloating()) {
      floatingFailed();
    } else {
      finishPipRequest();
      pauseIfHidden();
    }
  }, 3000);
  updateButtons();
  try {
    if (androidPip) {
      window.location.href = canvas.width >= canvas.height
        ? 'pocketcam://float/landscape' : 'pocketcam://float/portrait';
    } else if (document.pictureInPictureElement === video) {
      await document.exitPictureInPicture();
    } else if (video.webkitPresentationMode === 'picture-in-picture' || (ios && webKitPipSupported())) {
      video.webkitSetPresentationMode(opening ? 'picture-in-picture' : 'inline');
    } else if (document.pictureInPictureEnabled && typeof video.requestPictureInPicture === 'function') {
      await video.requestPictureInPicture();
    } else if (webKitPipSupported()) {
      video.webkitSetPresentationMode('picture-in-picture');
    } else {
      floatingFailed();
    }
  } catch {
    floatingFailed();
  }
}

function endStream(id) {
  request('stop', { method: 'POST', headers: { 'X-PocketCam-Stream': id }, keepalive: true }).catch(() => {});
}

function stopCamera() {
  const closeNative = nativePip || (androidPip && !!pendingPip);
  nativePip = false;
  document.body.classList.remove('floating-preview');
  finishPipRequest();
  if (closeNative) window.location.href = 'pocketcam://float-stopped';
  generation += 1;
  media?.getTracks().forEach((track) => track.stop());
  media = undefined;
  cameraSettings = Promise.resolve();
  flashOn = false;
  flashBusy = false;
  flashNote.hidden = true;
  if (document.pictureInPictureElement === video) document.exitPictureInPicture().catch(() => {});
  if (video.webkitPresentationMode === 'picture-in-picture') {
    try { video.webkitSetPresentationMode('inline'); } catch {}
  }
  if (socket) socket.close();
  socket = undefined;
  if (streamId) endStream(streamId);
  streamId = undefined;
  wakeLock?.release().catch(() => {});
  wakeLock = undefined;
  video.srcObject = null;
  video.hidden = true;
  canvas.hidden = true;
  modeLabel.hidden = true;
  floatNote.hidden = true;
  byId('camera-idle').hidden = false;
  updateButtons();
}

async function startCamera() {
  const current = ++generation;
  busy = true;
  updateButtons();
  message('Opening camera…', 'pending');
  try {
    const selected = picker.value;
    const preference = selected.startsWith('id:')
      ? { deviceId: { exact: selected.slice(3) } }
      : { facingMode: { ideal: selected } };
    const capture = await navigator.mediaDevices.getUserMedia({ audio: false, video: { ...constraints(mode), ...preference } });
    if (current !== generation) { capture.getTracks().forEach((track) => track.stop()); return; }
    media = capture;
    const track = capture.getVideoTracks()[0];
    track.addEventListener('mute', () => {
      if (current === generation) message('Your phone paused the camera. Return to WEBKAM to continue.', 'pending');
    });
    track.addEventListener('ended', () => {
      if (current !== generation) return;
      stopCamera();
      message('Camera stopped. Tap Start camera to reconnect.');
    });
    video.srcObject = capture;
    video.hidden = false;
    await video.play();
    listCameras();
    const response = await request('start', { method: 'POST' });
    if (!response.ok) {
      if (current === generation) { stopCamera(); await syncPairing(); }
      return;
    }
    const id = (await response.json()).stream;
    if (current !== generation) { endStream(id); return; }
    streamId = id;
    navigator.wakeLock?.request('screen').then((lock) => {
      if (current === generation) wakeLock = lock;
      else lock.release().catch(() => {});
    }).catch(() => {});
    connectVideo(current, id);
  } catch (error) {
    if (current !== generation) return;
    stopCamera();
    message(error.name === 'NotAllowedError'
      ? 'Allow camera access in this browser’s site settings, then try again.'
      : error.name === 'OverconstrainedError' || error.name === 'NotFoundError'
        ? 'This camera is unavailable. Choose another camera.'
        : 'Could not start the camera. Check WEBKAM on your PC and try again.');
  } finally {
    busy = false;
    updateButtons();
  }
}

function connectVideo(current, id) {
  if (current !== generation) return;
  const origin = location.origin.replace(/^http/, 'ws');
  const connection = new WebSocket(origin + '/api/video?stream=' + encodeURIComponent(id));
  socket = connection;
  let entered = false;
  message('Connecting to your PC…', 'pending');
  connection.addEventListener('message', (event) => {
    if (current !== generation || connection !== socket) return;
    try {
      setMode(JSON.parse(event.data));
      if (!entered) {
        entered = true;
        sendFrames(current, connection);
      }
    } catch {}
  });
  connection.addEventListener('close', (event) => {
    if (current !== generation || connection !== socket) return;
    if (event.code === 4001 || !entered) {
      stopCamera();
      syncPairing();
    } else {
      message('Reconnecting to your PC…', 'pending');
      setTimeout(() => connectVideo(current, id), 500);
    }
  });
}

function draw() {
  // Devices that rotate camera frames themselves already report the new aspect.
  // Use the display angle to turn frames from cameras with a fixed sensor aspect.
  const mobile = /Android|iPhone|iPad|iPod/.test(navigator.userAgent) ||
    (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);
  const displayAngle = mobile ? (window.orientation ?? window.screen.orientation?.angle) : undefined;
  const screenAngle = typeof displayAngle === 'number' ? (displayAngle + 360) % 360 : undefined;
  const videoWide = video.videoWidth > video.videoHeight;
  const screenWide = screenAngle === 90 || screenAngle === 270;
  if (screenWide) landscapeAngle = screenAngle;
  let automatic = 0;
  if (mode.follow_phone && screenAngle !== undefined && screenWide !== videoWide) {
    automatic = screenWide ? (360 - screenAngle) % 360 : (landscapeAngle + screenAngle) % 360;
  }
  const rotation = (automatic + (mode.rotation || 0)) % 360;
  const quarterTurn = rotation % 180 !== 0;
  const sourceWide = videoWide !== quarterTurn;
  const long = Math.max(mode.width, mode.height);
  const short = Math.min(mode.width, mode.height);
  const wide = mode.follow_phone ? sourceWide : mode.width > mode.height;
  const outputWidth = wide ? long : short;
  const outputHeight = wide ? short : long;
  if (canvas.width !== outputWidth || canvas.height !== outputHeight || canvas.hidden) {
    updateOutput(outputWidth, outputHeight);
    byId('camera-idle').hidden = true;
    canvas.hidden = false;
    modeLabel.hidden = false;
  }
  const width = quarterTurn ? outputHeight : outputWidth;
  const height = quarterTurn ? outputWidth : outputHeight;
  const scale = Math.max(width / video.videoWidth, height / video.videoHeight);
  context.save();
  context.translate(outputWidth / 2, outputHeight / 2);
  context.rotate(rotation * Math.PI / 180);
  context.drawImage(video, -video.videoWidth * scale / 2, -video.videoHeight * scale / 2,
    video.videoWidth * scale, video.videoHeight * scale);
  context.restore();
}

function sendFrames(current, connection) {
  const track = media?.getVideoTracks()[0];
  let nextFrame = 0;
  let mediaTime = -1;
  let encoding = false;
  const schedule = 'requestVideoFrameCallback' in video
    ? (callback) => video.requestVideoFrameCallback(callback)
    : (callback) => document.hidden && isFloating()
      ? setTimeout(() => callback(performance.now()), 1000 / mode.fps)
      : requestAnimationFrame(callback);

  function frame(now) {
    if (current !== generation || connection !== socket || connection.readyState !== WebSocket.OPEN) return;
    schedule(frame);
    const period = 1000 / mode.fps;
    if (track?.muted || !video.videoWidth || video.readyState < 2 || video.currentTime === mediaTime ||
        now + period * 0.32 < nextFrame) return;
    // Skip stale frames while encoding or waiting for the connection.
    if (encoding || connection.bufferedAmount > 450000) return;
    encoding = true;
    mediaTime = video.currentTime;
    nextFrame = Math.max(nextFrame + period, now + period * 0.52);
    const frameMode = mode;
    draw();

    function encode(quality) {
      canvas.toBlob((jpeg) => {
        if (!jpeg || current !== generation || connection !== socket || frameMode !== mode ||
            connection.readyState !== WebSocket.OPEN || connection.bufferedAmount > 750000) {
          encoding = false;
          return;
        }
        // Retry just this frame when it exceeds the receiver’s message limit.
        if (jpeg.size > MAX_JPEG_BYTES && quality === JPEG_QUALITY) {
          encode(LARGE_FRAME_QUALITY);
          return;
        }
        encoding = false;
        if (jpeg.size > MAX_JPEG_BYTES) return;
        connection.send(jpeg);
        if (connectionChip.dataset.state !== 'live') message('Live on your computer.', 'live');
      }, 'image/jpeg', quality);
    }
    encode(JPEG_QUALITY);
  }
  schedule(frame);
}

toggle.addEventListener('click', () => {
  if (media) { stopCamera(); message('Camera is off.'); }
  else startCamera();
});
picker.addEventListener('change', () => {
  if (media) {
    stopCamera();
    startCamera();
  }
});
navigator.mediaDevices?.addEventListener('devicechange', listCameras);
flash.addEventListener('click', toggleFlash);
floatCamera.addEventListener('click', toggleFloatingPreview);
video.addEventListener('loadeddata', updateButtons);
for (const event of ['enterpictureinpicture', 'leavepictureinpicture', 'webkitpresentationmodechanged']) {
  video.addEventListener(event, () => {
    finishPipRequest();
    if (!isFloating()) setTimeout(pauseIfHidden, 300);
  });
}
window.addEventListener('pocketcam-pip-entered', () => {
  nativePip = true;
  document.body.classList.add('floating-preview');
  finishPipRequest();
});
window.addEventListener('pocketcam-pip-exited', () => {
  nativePip = false;
  document.body.classList.remove('floating-preview');
  finishPipRequest();
  setTimeout(pauseIfHidden, 300);
});
window.addEventListener('pocketcam-pip-failed', () => {
  floatNote.textContent = 'Floating preview is unavailable. Check Picture-in-picture access for WEBKAM in Android Settings.';
  floatNote.hidden = false;
  finishPipRequest();
  pauseIfHidden();
});
window.addEventListener('pocketcam-backgrounded', () => {
  if (!media && !busy) return;
  stopCamera();
  message('Camera paused. Tap Start camera to reconnect.');
});
document.addEventListener('visibilitychange', pauseIfHidden);
window.addEventListener('pagehide', stopCamera);
