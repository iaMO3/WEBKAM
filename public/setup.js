const openCamera = document.getElementById('open-camera');
const message = document.getElementById('certificate-status');
const iphone = document.getElementById('iphone-setup');
const android = document.getElementById('android-setup');
const certificate = document.getElementById('certificate-name').textContent;
const isAndroid = /Android/i.test(navigator.userAgent);
const fromHomeScreen = location.pathname === '/iphone';

if (isAndroid) {
  iphone.open = false;
  android.open = true;
  document.getElementById('iphone-trust-help').hidden = true;
}

function checkCameraImage(url) {
  return new Promise((resolve) => {
    // The browser still validates TLS. The image probe avoids cross-origin
    // fetch restrictions on the HTTP setup page and sends no pairing information.
    const image = new Image();
    const finish = (ok) => {
      clearTimeout(timer);
      image.onload = image.onerror = null;
      resolve(ok);
    };
    const timer = setTimeout(() => finish(false), 5000);
    image.onload = () => finish(true);
    image.onerror = () => finish(false);
    image.src = url + '/icons/apple-touch-icon.png?check=' + Date.now();
  });
}

async function connect() {
  if (openCamera.disabled) return;
  const camera = openCamera.dataset.cameraUrl;
  openCamera.disabled = true;
  message.hidden = false;
  message.textContent = 'Checking the secure camera connection…';
  if (await checkCameraImage(camera)) {
    if (fromHomeScreen) window.location.replace(camera);
    else window.location.assign(camera);
    return;
  }
  openCamera.disabled = false;
  openCamera.textContent = 'Retry connection';
  if (isAndroid) {
    android.open = true;
    message.textContent = 'Keep WEBKAM open on Windows and install this PC’s certificate as a CA certificate on Android, then retry.';
  } else {
    iphone.open = true;
    message.textContent = 'Keep WEBKAM open on Windows, turn on full trust for ' + certificate + ', then tap Retry connection.';
  }
}

openCamera.addEventListener('click', connect);
if (fromHomeScreen) {
  document.getElementById('setup-intro').hidden = true;
  document.getElementById('profile-setup').hidden = true;
  connect();
}
