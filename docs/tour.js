const bar = document.querySelector(".tour-tabs");
const tabs = [...bar.querySelectorAll("a")];
const panels = tabs.map((tab) => document.querySelector(tab.hash));

function selectTab(selected) {
  tabs.forEach((tab, index) => {
    const active = tab === selected;
    tab.setAttribute("aria-selected", String(active));
    tab.tabIndex = active ? 0 : -1;
    panels[index].hidden = !active;
  });
}

bar.setAttribute("role", "tablist");
tabs.forEach((tab, index) => {
  tab.setAttribute("role", "tab");
  tab.setAttribute("aria-controls", panels[index].id);
  panels[index].setAttribute("role", "tabpanel");
  panels[index].setAttribute("aria-labelledby", tab.id);
  tab.addEventListener("click", (event) => {
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    selectTab(tab);
    history.replaceState(null, "", tab.hash);
  });
  tab.addEventListener("keydown", (event) => {
    const next = {
      ArrowLeft: (index + tabs.length - 1) % tabs.length,
      ArrowRight: (index + 1) % tabs.length,
      Home: 0,
      End: tabs.length - 1,
    }[event.key];
    if (next === undefined) return;
    event.preventDefault();
    tabs[next].focus();
    tabs[next].click();
  });
});
function selectLinkedPanel() {
  selectTab(tabs.find((tab) => tab.hash === location.hash) || tabs[0]);
}
selectLinkedPanel();
window.addEventListener("hashchange", selectLinkedPanel);

const setupViewer = document.querySelector(".setup-viewer");
if (typeof setupViewer.showModal === "function") {
  const previews = [...document.querySelectorAll(".step-art")];
  const figure = setupViewer.querySelector(".viewer-figure");
  const image = figure.querySelector("img");
  let currentStep = 0;

  function showStep(index) {
    currentStep = (index + previews.length) % previews.length;
    const preview = previews[currentStep];
    const step = preview.closest(".setup-step");
    image.src = preview.href;
    image.alt = preview.querySelector("img").alt;
    figure.querySelector(".camera-choice")?.remove();
    const choice = preview.querySelector(".camera-choice");
    if (choice) figure.append(choice.cloneNode(true));
    setupViewer.querySelector("#viewer-title").textContent = step.querySelector("h3").textContent;
    setupViewer.querySelector("#viewer-description").textContent = step.querySelector("p").textContent;
    setupViewer.querySelector(".viewer-position").textContent =
      `Step ${currentStep + 1} of ${previews.length}`;
  }

  previews.forEach((preview, index) => {
    preview.setAttribute("aria-haspopup", "dialog");
    preview.addEventListener("click", (event) => {
      if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
      event.preventDefault();
      showStep(index);
      setupViewer.showModal();
    });
  });
  setupViewer.querySelectorAll("[data-offset]").forEach((button) => {
    button.addEventListener("click", () => showStep(currentStep + Number(button.dataset.offset)));
  });
  setupViewer.addEventListener("keydown", (event) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    showStep(currentStep + (event.key === "ArrowLeft" ? -1 : 1));
  });
  setupViewer.addEventListener("click", (event) => {
    if (event.target !== setupViewer) return;
    const bounds = setupViewer.getBoundingClientRect();
    if (
      event.clientX < bounds.left ||
      event.clientX > bounds.right ||
      event.clientY < bounds.top ||
      event.clientY > bounds.bottom
    )
      setupViewer.close();
  });
}

// Sections are fully visible before enhancement, including when a visitor follows a deep link.
if ("IntersectionObserver" in window) {
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      entry.target.classList.add("in-view");
      observer.unobserve(entry.target);
    }
  });
  document
    .querySelectorAll(".section-head, .setup-heading, .setup-step, .download-card")
    .forEach((element) => observer.observe(element));
}

const desktopDemo = document.querySelector('[data-demo="desktop"]');
const desktopViewport = desktopDemo.querySelector(".demo-viewport");
const desktopStatus = desktopDemo.querySelector(".demo-telemetry");
let rotation = 0;

function updateDesktopDemo() {
  const resolution = desktopDemo.querySelector('[name="demo-resolution"]:checked').value;
  const fps = desktopDemo.querySelector('[name="demo-fps"]:checked').value;
  const landscape = rotation % 180 === 0;
  const dimensions = resolution === "1080" ? [1920, 1080] : [1280, 720];
  if (!landscape) dimensions.reverse();
  desktopDemo.dataset.resolution = resolution;
  desktopDemo.dataset.fps = fps;
  desktopViewport.style.setProperty("--demo-angle", `${rotation}deg`);
  desktopViewport.toggleAttribute("data-portrait", !landscape);
  const angle = rotation % 360;
  desktopStatus.textContent = `${dimensions.join(" × ")} · ${fps} FPS target${angle ? ` · ${angle}°` : ""}`;
}

desktopDemo.addEventListener("change", updateDesktopDemo);
desktopDemo.querySelector(".demo-rotate").addEventListener("click", () => {
  rotation += 90;
  updateDesktopDemo();
});

const phoneDemo = document.querySelector('[data-demo="phone"]');
const phoneStatus = phoneDemo.querySelector(".demo-signal");
const phoneButton = phoneDemo.querySelector(".demo-stream");
const cameraDemos = [desktopDemo, phoneDemo];
let facing = "rear";
let lightOn = false;

function renderLighting() {
  for (const demo of cameraDemos) {
    demo.querySelector(`.demo-light-controls input[value="${facing}"]`).checked = true;
    const button = demo.querySelector(".demo-light");
    button.disabled = phoneDemo.dataset.state === "idle";
    button.setAttribute("aria-pressed", String(lightOn));
    button.querySelector(".demo-light-label").textContent = facing === "rear" ? "Flash" : "Screen light";
    button.querySelector(".demo-light-state").textContent = lightOn ? "On" : "Off";
    demo.dataset.light = lightOn ? facing : "off";
  }
}

for (const demo of cameraDemos) {
  const controls = demo.querySelector(".demo-light-controls");
  controls.append(document.querySelector("#demo-light-controls").content.cloneNode(true));
  controls.querySelectorAll("input").forEach(input => {
    input.name = `demo-camera-${demo.dataset.demo}`;
    input.addEventListener("change", () => {
      facing = input.value;
      lightOn = false;
      renderLighting();
    });
  });
  controls.querySelector(".demo-light").addEventListener("click", () => {
    lightOn = !lightOn;
    renderLighting();
  });
}
renderLighting();

function updatePhoneLens() {
  const lens = phoneDemo.querySelector('[name="demo-lens"]:checked').value;
  phoneDemo.style.setProperty("--demo-zoom", lens);
}
phoneDemo.addEventListener("change", updatePhoneLens);
updatePhoneLens();
phoneButton.addEventListener("click", () => {
  const running = phoneDemo.dataset.state === "idle";
  phoneDemo.dataset.state = running ? "running" : "idle";
  phoneStatus.textContent = running ? "Preview" : "Stopped";
  phoneDemo.querySelector(".demo-phone-idle").hidden = running;
  phoneButton.textContent = running ? "Stop sample" : "Start sample";
  lightOn = false;
  renderLighting();
});

const pairingDemo = document.querySelector('[data-demo="pairing"]');
const routeHelp = pairingDemo.querySelector(".demo-route-help");
const routeIcon = pairingDemo.querySelector(".demo-route-line use");
const pairButton = pairingDemo.querySelector(".demo-connect");
const routeInstructions = {
  wifi: "Join the same network, then scan the QR code shown in WEBKAM.",
  iphone: "Plug in your iPhone, trust this computer, and turn on Personal Hotspot. Scan the QR code in WEBKAM.",
  android: "Plug in your Android phone. On Windows, turn on USB tethering or use USB debugging with the native companion.",
};

function updatePairingDemo(paired) {
  const route = pairingDemo.querySelector('[name="demo-route"]:checked').value;
  pairingDemo.dataset.state = paired ? "paired" : "idle";
  pairingDemo.querySelector(".demo-signal").textContent = paired ? "Sample phone paired" : "Ready to pair";
  pairButton.textContent = paired ? "Disconnect sample phone" : "Pair sample phone";
  routeHelp.textContent = paired
    ? "The phone can now start streaming. Select WEBKAM as the camera in your Windows app."
    : routeInstructions[route];
  routeIcon.setAttribute("href", paired ? "#check" : route === "wifi" ? "#wifi" : "#usb");
}

pairButton.addEventListener("click", () => updatePairingDemo(pairingDemo.dataset.state !== "paired"));
pairingDemo.addEventListener("change", () => updatePairingDemo(false));
updatePairingDemo(false);
document.querySelectorAll(".demo-controls").forEach((controls) => (controls.hidden = false));
